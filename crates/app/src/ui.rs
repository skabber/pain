//! A right-click context menu for grouping/broadcast-mode control, plus the
//! settings panel it opens into.
//!
//! Group assignment and broadcast-mode selection live here, on demand,
//! rather than as keybindings or a permanently visible panel: Terminator
//! itself only exposes group assignment through its GUI, never a
//! keybinding or a persistent widget — its own right-click menu is the
//! precedent this follows. A first attempt used an always-visible floating
//! `egui::Window`; that's the wrong chrome pattern (screen furniture that's
//! in the way even when nobody's touching it), so this replaced it with a
//! menu that only exists between a right-click and the next action.
//!
//! The settings panel (Milestone 5.4) follows the same "on demand, not
//! furniture" rule but is a different kind of chrome: it's a form you
//! explicitly open and close, the same as Terminator's own Preferences
//! dialog (itself reached through that same right-click menu, not a
//! separate menu bar this app doesn't have) — an `egui::Window` is the
//! right container for that, unlike for the always-on broadcast controls.

use layout::{Arrangement, Orientation, PaneId};
use router::BroadcastMode;
use winit::event::WindowEvent;
use winit::window::Window;

pub struct Ui {
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    /// The pane the open pane-management context menu targets, and where to
    /// draw it — opened by right-clicking a pane's *title bar* specifically
    /// (see `Graphics::pane_title_bar_at`). Right-clicking a pane always
    /// targets *that* pane, not necessarily the focused one — matching
    /// Terminator's per-terminal context menu.
    context_menu: Option<(PaneId, egui::Pos2)>,
    /// The pane the open terminal (copy/paste) context menu targets, and
    /// where to draw it — opened by right-clicking anywhere in a pane's
    /// terminal content instead of its title bar. Mutually exclusive with
    /// `context_menu`: opening either one clears the other, so only one
    /// menu is ever on screen at a time.
    terminal_context_menu: Option<(PaneId, egui::Pos2)>,
    /// The new-group-name text field's current contents, while a context
    /// menu is open. Reset whenever a menu opens or closes so stale text
    /// from one pane's menu can't leak into another's.
    group_name_input: String,
    /// The "swap shell" text field's current contents, while a context menu
    /// is open. Reset the same way and for the same reason as
    /// `group_name_input`.
    swap_shell_input: String,
    /// The settings panel's in-progress edits, if it's open. `None` means
    /// closed — there's no separate open/closed flag to keep in sync with
    /// this.
    settings_panel: Option<SettingsDraft>,
    /// A paste awaiting the user's confirmation: the target pane and the
    /// full clipboard text. Held here (not re-read from the clipboard on
    /// confirm) so what gets sent is exactly what was described in the
    /// prompt, even if the clipboard changes while the dialog is open.
    paste_confirm: Option<(PaneId, String)>,
}

/// What the user asked for by interacting with the menu this frame.
#[derive(Default)]
pub struct UiRequest {
    pub set_broadcast_mode: Option<BroadcastMode>,
    /// Split the given pane in the given orientation — the context menu's
    /// target pane, not necessarily the focused one (see `open_context_menu`).
    pub split: Option<(PaneId, Orientation)>,
    /// Assign the given pane to the named group, creating it if it's new.
    pub assign_to_group: Option<(PaneId, String)>,
    pub remove_from_group: Option<PaneId>,
    /// Kill the given pane's current shell and start a fresh one in its
    /// place, leaving the pane itself (position, group, broadcast
    /// membership) untouched. `None` means the platform default, same
    /// convention as `Graphics::shell`. Exists for cases like `wsl.exe`
    /// launched from inside a Windows shell, where the pane's foreground-
    /// process detection can't see past that boundary (see
    /// `foreground_process`'s doc comment) — swapping directly into the
    /// nested shell sidesteps the problem instead of detecting it.
    pub restart_shell: Option<(PaneId, Option<String>)>,
    /// Rearrange every pane currently open into a preset shape — see
    /// `layout::Arrangement`. Not scoped to the context menu's target
    /// pane like the other actions above; this always acts on the whole
    /// layout regardless of which pane was right-clicked.
    pub arrange: Option<layout::Arrangement>,
    /// Copy the given pane's current selection to the system clipboard —
    /// from the terminal context menu's "Copy", not the pane-management
    /// one.
    pub copy_selection: Option<PaneId>,
    /// Write the system clipboard's text straight to the given pane's
    /// shell, as if typed — the terminal context menu's "Paste".
    pub paste_clipboard: Option<PaneId>,
    /// Close the given pane — from either right-click menu's "Close" (the
    /// pane-management menu's or the terminal menu's), not the title-bar
    /// close button, which acts directly through `Graphics::close_button_at`
    /// instead of round-tripping through a request.
    pub close_pane: Option<PaneId>,
    /// The settings panel's Save button was clicked, carrying the fully
    /// resolved config that was just written to disk — `Graphics` applies
    /// it live (same as it's already been doing for the in-progress
    /// preview) and remembers it as the new "last saved" baseline to
    /// revert to on a future Cancel.
    pub settings_saved: Option<config::Config>,
    /// The settings panel was closed *without* saving — Cancel, or the
    /// window's own close button — so whatever was being live-previewed
    /// should revert to the last saved config.
    pub settings_cancelled: bool,
    /// The user approved a paste that had been held for confirmation —
    /// carries the exact text that was shown in the prompt.
    pub confirm_paste: Option<(PaneId, String)>,
}

/// The settings panel's editable fields, seeded from the live `Config` when
/// opened and discarded (not applied) unless Save is clicked. Deliberately
/// a separate struct from `config::Config` rather than editing a clone of
/// it directly — most widgets below need a plain `&mut f32`/`&mut String`,
/// and keeping the draft's shape flat avoids threading `&mut
/// config.appearance.font_size`-style paths through egui widget calls.
struct SettingsDraft {
    font_family: String,
    font_size: f32,
    transparency: f32,
    background_color: [f32; 3],
    accent_color: [f32; 3],
    scrollback_lines: usize,
    default_shell: String,
    cursor_style: config::CursorStyle,
}

impl SettingsDraft {
    fn from_config(config: &config::Config) -> Self {
        Self {
            font_family: config.appearance.font_family.clone(),
            font_size: config.appearance.font_size,
            transparency: config.appearance.transparency,
            background_color: config.appearance.background_rgb(),
            accent_color: config.appearance.accent_rgb(),
            scrollback_lines: config.general.scrollback_lines,
            default_shell: config.general.default_shell.clone(),
            cursor_style: config.cursor.style,
        }
    }

    /// Applies the draft's edits onto a clone of `base` — anything the
    /// panel doesn't expose (theme, keybinding overrides) passes through
    /// untouched, so saving from the panel can never silently drop a
    /// hand-edited setting the panel has no field for.
    fn apply_to(&self, base: &config::Config) -> config::Config {
        let mut config = base.clone();
        config.appearance.set_background_rgb(self.background_color);
        config.appearance.set_accent_rgb(self.accent_color);
        config.appearance.font_family = self.font_family.clone();
        config.appearance.font_size = self.font_size;
        config.appearance.transparency = self.transparency;
        config.general.scrollback_lines = self.scrollback_lines;
        config.general.default_shell = self.default_shell.clone();
        config.cursor.style = self.cursor_style;
        config
    }
}

impl Ui {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat, window: &Window) -> Self {
        let ctx = egui::Context::default();
        install_chrome_font(&ctx);
        apply_chrome_style(&ctx);
        let state = egui_winit::State::new(ctx.clone(), egui::ViewportId::ROOT, window, None, None, None);
        let renderer = egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());
        Self {
            ctx,
            state,
            renderer,
            context_menu: None,
            terminal_context_menu: None,
            group_name_input: String::new(),
            swap_shell_input: String::new(),
            settings_panel: None,
            paste_confirm: None,
        }
    }

    /// Feeds a window event to egui. Returns whether egui consumed it —
    /// callers should not also treat the event as pane/divider input when
    /// this is true (e.g. a click landing on the menu shouldn't also focus
    /// whatever pane happens to be underneath it).
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.state.on_window_event(window, event).consumed
    }

    /// Whether the pane-management menu, the terminal context menu, or the
    /// settings panel currently has something open. While any of them is,
    /// `Tab` should behave as normal egui focus-cycling between their own
    /// widgets (e.g. the group-name field) rather than falling through to
    /// the pane underneath — see `main.rs`'s Tab-key handling for why this
    /// matters.
    pub fn wants_keyboard_focus(&self) -> bool {
        self.context_menu.is_some()
            || self.terminal_context_menu.is_some()
            || self.settings_panel.is_some()
            || self.paste_confirm.is_some()
    }

    /// The config that would result from applying the settings panel's
    /// in-progress edits on top of `base`, if the panel is open — for
    /// `Graphics::redraw` to render against live, every frame, instead of
    /// only once Save is clicked. `None` (not just "unchanged") when the
    /// panel is closed, so the caller can tell "nothing to preview" apart
    /// from "preview happens to equal the current settings."
    pub fn live_preview(&self, base: &config::Config) -> Option<config::Config> {
        self.settings_panel.as_ref().map(|draft| draft.apply_to(base))
    }

    /// Opens the pane-management context menu for `pane` at `pos` (window
    /// pixel coordinates), replacing whichever menu (if any) was already
    /// open — including the terminal context menu, since only one of the
    /// two is ever shown at a time.
    pub fn open_context_menu(&mut self, pane: PaneId, pos: (f32, f32)) {
        self.context_menu = Some((pane, egui::pos2(pos.0, pos.1)));
        self.terminal_context_menu = None;
        self.group_name_input.clear();
        self.swap_shell_input.clear();
    }

    /// Opens the terminal (copy/paste) context menu for `pane` at `pos`,
    /// replacing whichever menu (if any) was already open — see
    /// `open_context_menu`'s note on mutual exclusivity.
    pub fn open_terminal_context_menu(&mut self, pane: PaneId, pos: (f32, f32)) {
        self.terminal_context_menu = Some((pane, egui::pos2(pos.0, pos.1)));
        self.context_menu = None;
    }

    /// Holds `text` pending the user's approval before it's pasted into
    /// `pane` — see `crate::paste::needs_confirmation` for when this is
    /// used instead of pasting straight away.
    pub fn open_paste_confirm(&mut self, pane: PaneId, text: String) {
        self.paste_confirm = Some((pane, text));
    }

    /// Closes whichever context menu is open, if either is. Returns
    /// whether one was.
    pub fn close_context_menu(&mut self) -> bool {
        let closed_pane_menu = self.context_menu.take().is_some();
        let closed_terminal_menu = self.terminal_context_menu.take().is_some();
        closed_pane_menu || closed_terminal_menu
    }

    /// Runs the menu for one frame (if one is open) and returns what the
    /// user asked for, plus the render output to composite via
    /// `Ui::render`. `current_group` reports the target pane's group name,
    /// if it's in one; `group_names` lists every group that currently has
    /// at least one member, for the "add to an existing group" picker.
    pub fn show(
        &mut self,
        window: &Window,
        broadcast_mode: BroadcastMode,
        current_group: impl Fn(PaneId) -> Option<String>,
        group_names: &[&str],
        settings: &config::Config,
    ) -> (UiRequest, egui::FullOutput) {
        // Cheap to set unconditionally every frame — unlike fonts (an atlas
        // rebuild), a `Visuals` swap is just plain data, so there's no need
        // to track whether the accent color actually changed since last
        // frame.
        self.ctx.set_visuals(graphite_visuals(settings.appearance.accent_rgb()));

        let raw_input = self.state.take_egui_input(window);
        let mut request = UiRequest::default();
        // `context_menu`'s position was captured from winit's `CursorMoved`,
        // in physical pixels — the same unit everything else in this app
        // uses (layout rects, hit-testing, ...). egui positions things in
        // logical points instead, so it has to be converted here, at the
        // boundary, rather than changing what unit the rest of the app
        // works in. Skipping this only "worked" on displays with a 1.0
        // scale factor (physical == logical there by coincidence) — it
        // drifted proportionally to distance from the origin on a scaled
        // Windows display, which is exactly what a missing unit conversion
        // looks like.
        let scale = egui_winit::pixels_per_point(&self.ctx, window);
        let context_menu = self.context_menu.map(|(pane, pos)| (pane, egui::pos2(pos.x / scale, pos.y / scale)));
        let terminal_context_menu =
            self.terminal_context_menu.map(|(pane, pos)| (pane, egui::pos2(pos.x / scale, pos.y / scale)));
        let mut close_after = false;
        let mut close_terminal_after = false;
        // Moved out of `self` for the duration of the closure below, same
        // reason `close_after` exists: `self.ctx.run_ui` already holds
        // `self.ctx`, so nothing inside the closure can also reach into
        // `self.settings_panel`/`self.group_name_input` directly. Opening
        // the settings panel (from the "Settings..." item below) just
        // assigns into the local, which the render code right after it in
        // the same closure picks up immediately — no extra frame of delay
        // before it appears.
        let mut settings_draft = self.settings_panel.take();
        let paste_confirm = self.paste_confirm.take();
        let mut paste_confirm_handled = false;
        let mut close_settings_panel = false;
        let mut group_name_input = core::mem::take(&mut self.group_name_input);
        let mut swap_shell_input = core::mem::take(&mut self.swap_shell_input);

        // `run_ui`, not `begin_pass`/`end_pass`: the latter never sets
        // egui's internal `root_ui_available_rect` (that's only populated
        // by `run_ui`'s root-Ui bookkeeping), which makes
        // `Context::is_pointer_over_egui` — and so `on_window_event`'s
        // `consumed` flag — fall into an explicit "shouldn't get here, but
        // who knows" fallback that returns `true` unconditionally,
        // anywhere in the window, menu open or not. That silently ate
        // every mouse press (right-click to open the menu, left-click to
        // start a divider drag) before it reached our own handling —
        // found by reading egui's actual source, not guessed.
        let full_output = self.ctx.run_ui(raw_input, |ui| {
            let ctx = ui.ctx().clone();

            if let Some((pane, pos)) = context_menu {
                let pane_group = current_group(pane);
                let accent_color32 = color32_from_rgb(settings.appearance.accent_rgb());
                egui::Area::new(egui::Id::new("pane-context-menu"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .show(&ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            // A fixed width, not just a minimum: `Area`
                            // doesn't otherwise bound how much horizontal
                            // space is "available" to its content, so
                            // `ui.columns(...)` below (which divides
                            // *whatever* `available_width()` reports) was
                            // stretching every button across nearly the
                            // whole window instead of a compact ~240px
                            // menu — a real, confirmed bug (read via
                            // egui's own layout source), not a hunch.
                            ui.set_width(240.0);
                            section_header(ui, "Broadcast");
                            // A horizontal radio row, not a vertical
                            // selectable-list: only one mode is ever active
                            // at once, which a radio group communicates
                            // more directly than a list of individually
                            // clickable pills.
                            ui.horizontal(|ui| {
                                for (mode, label) in [
                                    (BroadcastMode::Off, "Off"),
                                    (BroadcastMode::Group, "Group"),
                                    (BroadcastMode::All, "All"),
                                ] {
                                    let active = broadcast_mode == mode;
                                    // The active mode's label itself turns
                                    // accent-colored, not just its dot —
                                    // matching the mockup's `.radio.active`
                                    // rule (`override_text_color` would
                                    // otherwise force every label to the
                                    // same ink color regardless of state).
                                    let text = egui::RichText::new(label).color(if active { accent_color32 } else { MUTED });
                                    if ui.radio(active, text).clicked() {
                                        request.set_broadcast_mode = Some(mode);
                                        close_after = true;
                                    }
                                }
                            });

                            ui.separator();
                            section_header(ui, "Split");
                            ui.columns(2, |cols| {
                                if cols[0]
                                    .add_sized([cols[0].available_width(), 0.0], egui::Button::new("Horizontal"))
                                    .clicked()
                                {
                                    request.split = Some((pane, Orientation::Horizontal));
                                    close_after = true;
                                }
                                if cols[1]
                                    .add_sized([cols[1].available_width(), 0.0], egui::Button::new("Vertical"))
                                    .clicked()
                                {
                                    request.split = Some((pane, Orientation::Vertical));
                                    close_after = true;
                                }
                            });

                            ui.separator();
                            section_header(ui, "Arrange all panes");
                            ui.columns(3, |cols| {
                                if cols[0]
                                    .add_sized([cols[0].available_width(), 0.0], egui::Button::new("Horizontal"))
                                    .clicked()
                                {
                                    request.arrange = Some(Arrangement::Horizontal);
                                    close_after = true;
                                }
                                if cols[1]
                                    .add_sized([cols[1].available_width(), 0.0], egui::Button::new("Vertical"))
                                    .clicked()
                                {
                                    request.arrange = Some(Arrangement::Vertical);
                                    close_after = true;
                                }
                                if cols[2].add_sized([cols[2].available_width(), 0.0], egui::Button::new("Grid")).clicked()
                                {
                                    request.arrange = Some(Arrangement::Grid);
                                    close_after = true;
                                }
                            });

                            ui.separator();
                            section_header(ui, "Group");
                            if let Some(name) = &pane_group {
                                ui.horizontal(|ui| {
                                    ui.label("In group");
                                    ui.label(egui::RichText::new(name).strong());
                                });
                                if ui.button("Remove from group").clicked() {
                                    request.remove_from_group = Some(pane);
                                    close_after = true;
                                }
                            }
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut group_name_input)
                                        .hint_text("New group name")
                                        .desired_width(120.0),
                                );
                                let name = group_name_input.trim();
                                if ui.add_enabled(!name.is_empty(), egui::Button::new("Add")).clicked() {
                                    request.assign_to_group = Some((pane, name.to_string()));
                                    close_after = true;
                                }
                            });
                            if !group_names.is_empty() {
                                egui::ComboBox::from_label("Existing group")
                                    .selected_text("Choose...")
                                    .show_ui(ui, |ui| {
                                        for name in group_names {
                                            if ui.selectable_label(false, *name).clicked() {
                                                request.assign_to_group = Some((pane, (*name).to_string()));
                                                close_after = true;
                                            }
                                        }
                                    });
                            }

                            ui.separator();
                            section_header(ui, "Swap shell");
                            // Windows-only quick picks, same three presets
                            // (and the same rationale — no single obvious
                            // default the way Unix has `$SHELL`) as the
                            // settings panel's "Quick pick" row below.
                            // Unlike that row, picking one here acts
                            // immediately instead of filling a draft field
                            // for a separate Save step — this menu has no
                            // "cancel" concept, so there's nothing to
                            // stage.
                            #[cfg(target_os = "windows")]
                            {
                                let mut clicked_shell = None;
                                ui.columns(3, |cols| {
                                    if cols[0].add_sized([cols[0].available_width(), 0.0], egui::Button::new("cmd")).clicked() {
                                        clicked_shell = Some("cmd.exe");
                                    }
                                    if cols[1]
                                        .add_sized([cols[1].available_width(), 0.0], egui::Button::new("PowerShell"))
                                        .clicked()
                                    {
                                        clicked_shell = Some("powershell.exe");
                                    }
                                    if cols[2].add_sized([cols[2].available_width(), 0.0], egui::Button::new("WSL")).clicked() {
                                        clicked_shell = Some("wsl.exe");
                                    }
                                });
                                if let Some(shell) = clicked_shell {
                                    request.restart_shell = Some((pane, Some(shell.to_string())));
                                    close_after = true;
                                }
                            }
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut swap_shell_input)
                                        .hint_text("(platform default)")
                                        .desired_width(120.0),
                                );
                                let shell = swap_shell_input.trim();
                                let label = if shell.is_empty() { "Restart" } else { "Swap" };
                                if ui.button(label).clicked() {
                                    let shell = (!shell.is_empty()).then(|| shell.to_string());
                                    request.restart_shell = Some((pane, shell));
                                    close_after = true;
                                }
                            });

                            ui.separator();
                            section_header(ui, "Pane");
                            if ui.button("Close").clicked() {
                                request.close_pane = Some(pane);
                                close_after = true;
                            }

                            ui.separator();
                            ui.add_space(2.0);
                            // A real (framed) button, not a frameless
                            // "link" style: the frameless version's text
                            // color was hardcoded to `MUTED` via
                            // `RichText`, which always wins over the
                            // widget-state-driven hover color our theme
                            // otherwise provides — so it never visibly
                            // reacted to hover at all. A plain button gets
                            // that hover feedback for free from the same
                            // theming every other button in this menu
                            // already uses.
                            if ui.button("Settings...").clicked() {
                                settings_draft = Some(SettingsDraft::from_config(settings));
                                close_after = true;
                            }
                        });
                    });
            }

            if let Some((pane, pos)) = terminal_context_menu {
                egui::Area::new(egui::Id::new("terminal-context-menu"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .show(&ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.set_width(140.0);
                            if ui.button("Copy").clicked() {
                                request.copy_selection = Some(pane);
                                close_terminal_after = true;
                            }
                            if ui.button("Paste").clicked() {
                                request.paste_clipboard = Some(pane);
                                close_terminal_after = true;
                            }
                            ui.separator();
                            if ui.button("Close").clicked() {
                                request.close_pane = Some(pane);
                                close_terminal_after = true;
                            }
                        });
                    });
            }

            if let Some((pane, text)) = &paste_confirm {
                // Modal-ish: `Order::Foreground` plus a centered window,
                // deliberately without a close button — the two explicit
                // choices below are the only ways out, so a stray click on
                // an `X` can't silently drop a paste the user meant to send.
                egui::Window::new("Confirm paste")
                    .order(egui::Order::Foreground)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(&ctx, |ui| {
                        ui.set_width(420.0);
                        section_header(ui, "This paste will run immediately");
                        ui.label(
                            "The program in this pane hasn't enabled bracketed paste, so every \
                             line break below runs as a command the moment it arrives.",
                        );
                        ui.add_space(6.0);
                        ui.label(egui::RichText::new(crate::paste::summarize(text)).monospace().color(MUTED));
                        ui.add_space(4.0);
                        // A scrollable, read-only view of exactly what will
                        // be sent — the whole point of the prompt is that
                        // the user can actually look at it first.
                        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(&mut text.as_str())
                                    .desired_width(f32::INFINITY)
                                    .font(egui::TextStyle::Monospace)
                                    .interactive(false),
                            );
                        });
                        ui.separator();
                        action_row(ui, |ui| {
                            if ui.button("Paste anyway").clicked() {
                                request.confirm_paste = Some((*pane, text.clone()));
                                paste_confirm_handled = true;
                            }
                            if ui.button("Cancel").clicked() {
                                paste_confirm_handled = true;
                            }
                        });
                    });
            }

            if let Some(draft) = &mut settings_draft {
                let mut still_open = true;
                // Not collapsible: the mockup's panel header is a plain
                // title, not a section that toggles away — the default
                // collapse triangle is stock egui window chrome this
                // design pass otherwise moved away from. `default_width`
                // matters here beyond cosmetics: left at egui's own
                // content-fit default, every field below rendered at its
                // bare intrinsic size (a tiny color swatch, a narrow drag
                // value) instead of the mockup's wide, aligned field grid —
                // a real, visible gap a developer screenshot caught.
                egui::Window::new("Settings")
                    .collapsible(false)
                    .resizable(false)
                    .default_width(420.0)
                    .open(&mut still_open)
                    .show(&ctx, |ui| {
                    // Proportional, not pixel-fixed: a fraction of
                    // whatever the window's *actual current* width is,
                    // recomputed every frame, rather than hardcoded
                    // absolute numbers. The window is resizable again
                    // (a fixed-size window has no real use for "flex"),
                    // so this is what keeps the label/control ratio
                    // looking the same whether the developer drags it
                    // wider or it renders on a different sized display.
                    // Reading `available_width()` here — once, at the
                    // top of the window's own content `Ui`, not nested
                    // inside a `Grid` cell — is exactly what's safe about
                    // it: this is a real, already-settled width (the
                    // window's), unlike the runaway values that came from
                    // calling it deep inside a `Grid`/`Area` before their
                    // own size was known.
                    let content_width = ui.available_width();
                    let label_width = (content_width * LABEL_COL_FRACTION).clamp(80.0, 160.0);
                    let value_width = (content_width - label_width - GRID_COLUMN_GAP).max(80.0);

                    section_header(ui, "Appearance");
                    egui::Grid::new("settings-appearance").num_columns(2).spacing([GRID_COLUMN_GAP, 9.0]).show(
                        ui,
                        |ui| {
                            grid_label(ui, "Font", label_width);
                            let selected = if draft.font_family.is_empty() {
                                "monospace (system default)"
                            } else {
                                &draft.font_family
                            };
                            egui::ComboBox::from_id_salt("font-family")
                                .width(value_width)
                                .selected_text(selected)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut draft.font_family,
                                        String::new(),
                                        "monospace (system default)",
                                    );
                                    for name in render::monospace_font_families() {
                                        ui.selectable_value(&mut draft.font_family, name.clone(), name.as_str());
                                    }
                                });
                            ui.end_row();

                            grid_label(ui, "Size", label_width);
                            slider_field(ui, value_width, egui::Slider::new(&mut draft.font_size, 6.0..=48.0));
                            ui.end_row();

                            grid_label(ui, "Background", label_width);
                            color_field(ui, value_width, &mut draft.background_color);
                            ui.end_row();

                            grid_label(ui, "Accent", label_width);
                            color_field(ui, value_width, &mut draft.accent_color);
                            ui.end_row();
                        },
                    );

                    ui.separator();
                    section_header(ui, "Terminal");
                    egui::Grid::new("settings-terminal").num_columns(2).spacing([GRID_COLUMN_GAP, 9.0]).show(
                        ui,
                        |ui| {
                        grid_label(ui, "Transparency", label_width);
                        // Disabled, not hidden, on WSL: the setting still
                        // saves and applies normally on the platforms that
                        // actually support it (Windows, native Linux) —
                        // WSLg's compositor doesn't handle real window
                        // transparency correctly (see `platform::is_wsl`'s
                        // doc comment), and WSL isn't a target platform
                        // here, just a dev environment, so this is
                        // disabled outright rather than left to silently
                        // do nothing when dragged.
                        ui.add_enabled_ui(!crate::platform::is_wsl(), |ui| {
                            slider_field(ui, value_width, egui::Slider::new(&mut draft.transparency, 0.0..=1.0));
                        });
                        ui.end_row();

                        grid_label(ui, "Scrollback", label_width);
                        field_box(ui, value_width, |ui| {
                            ui.add(egui::DragValue::new(&mut draft.scrollback_lines).range(0..=1_000_000usize).suffix(" lines"));
                        });
                        ui.end_row();

                        grid_label(ui, "Cursor", label_width);
                        // A stretched segmented control (`ui.columns` +
                        // `add_sized`), not a plain `ui.horizontal` of
                        // `selectable_value`s — matching the mockup's
                        // `.segmented` row, whose three buttons use
                        // `flex: 1` to fill the full column width instead
                        // of shrinking to their own label text.
                        let mut clicked_style = None;
                        ui.columns(3, |cols| {
                            for (col, (style, label)) in cols.iter_mut().zip([
                                (config::CursorStyle::Block, "Block"),
                                (config::CursorStyle::Underline, "Underline"),
                                (config::CursorStyle::Beam, "Beam"),
                            ]) {
                                let selected = draft.cursor_style == style;
                                if col
                                    .add_sized([col.available_width(), 0.0], egui::Button::selectable(selected, label))
                                    .clicked()
                                {
                                    clicked_style = Some(style);
                                }
                            }
                        });
                        if let Some(style) = clicked_style {
                            draft.cursor_style = style;
                        }
                        ui.end_row();
                    });
                    if crate::platform::is_wsl() {
                        ui.weak("Transparency isn't supported under WSL.");
                    }

                    ui.separator();
                    section_header(ui, "Shell");
                    egui::Grid::new("settings-shell").num_columns(2).spacing([GRID_COLUMN_GAP, 9.0]).show(ui, |ui| {
                        grid_label(ui, "Default", label_width);
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.default_shell)
                                .hint_text("(platform default)")
                                .desired_width(value_width),
                        );
                        ui.end_row();
                    });
                    // Windows-only: unlike Linux/macOS (one obvious choice
                    // — whatever `$SHELL`/the OS already has configured,
                    // which leaving this field empty already picks up),
                    // Windows has no single obvious default shell — cmd,
                    // Windows PowerShell, and WSL are all common, equally
                    // reasonable choices, and typing an exact executable
                    // name/path into the field above is real friction
                    // compared to picking one. These just fill that field
                    // in; the field itself still takes any custom value (a
                    // specific WSL distro invocation, `pwsh.exe`, ...).
                    //
                    // A full-width row below the grid, not a third grid
                    // row: the mockup's own `.quick-picks` sits outside
                    // `.field-grid` as its own sibling, spanning the whole
                    // section instead of being squeezed into just the
                    // value column — confirmed by re-reading the mockup's
                    // HTML directly rather than assuming.
                    #[cfg(target_os = "windows")]
                    {
                        ui.add_space(2.0);
                        ui.columns(3, |cols| {
                            if cols[0]
                                .add_sized([cols[0].available_width(), 0.0], egui::Button::new("Command Prompt"))
                                .clicked()
                            {
                                draft.default_shell = "cmd.exe".to_string();
                            }
                            if cols[1]
                                .add_sized([cols[1].available_width(), 0.0], egui::Button::new("PowerShell"))
                                .clicked()
                            {
                                draft.default_shell = "powershell.exe".to_string();
                            }
                            if cols[2].add_sized([cols[2].available_width(), 0.0], egui::Button::new("WSL")).clicked() {
                                draft.default_shell = "wsl.exe".to_string();
                            }
                        });
                    }

                    ui.separator();
                    section_header(ui, "Keybindings");
                    // Read-only: hand-edit `config.toml`'s `[keybindings]`
                    // to change these (Milestone 5.3) — remapping chords
                    // from inside the panel is future polish, not something
                    // 5.4's own acceptance criteria call for.
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        if settings.keybindings.is_empty() {
                            ui.weak("(none — using built-in defaults)");
                        }
                        for (chord, action) in &settings.keybindings {
                            ui.label(format!("{chord}  \u{2192}  {action}"));
                        }
                    });
                    ui.separator();
                    // `right_to_left`: the mockup's action row is flush
                    // against the panel's right edge (Cancel, then Save
                    // at the very edge), not left-packed like a plain
                    // `ui.horizontal` would render it — the first widget
                    // added under `right_to_left` lands rightmost, so Save
                    // is added first here despite reading second on
                    // screen.
                    action_row(ui, |ui| {
                        if ui.button("Save").clicked() {
                            let new_config = draft.apply_to(settings);
                            if let Err(err) = new_config.save(&config::Config::default_path()) {
                                eprintln!("config: failed to save settings: {err:#}");
                            }
                            request.settings_saved = Some(new_config);
                            close_settings_panel = true;
                        }
                        if ui.button("Cancel").clicked() {
                            request.settings_cancelled = true;
                            close_settings_panel = true;
                        }
                    });
                });
                if !still_open {
                    // The window's own close button, not either of our
                    // two — same as Cancel: closed without an explicit
                    // Save, so whatever was being live-previewed should
                    // revert.
                    request.settings_cancelled = true;
                    close_settings_panel = true;
                }
            }
        });

        if close_after {
            self.context_menu = None;
            self.group_name_input = String::new();
            self.swap_shell_input = String::new();
        } else {
            self.group_name_input = group_name_input;
            self.swap_shell_input = swap_shell_input;
        }
        if close_terminal_after {
            self.terminal_context_menu = None;
        }
        self.settings_panel = if close_settings_panel { None } else { settings_draft };
        if !paste_confirm_handled {
            self.paste_confirm = paste_confirm;
        }

        self.state.handle_platform_output(window, full_output.platform_output.clone());
        (request, full_output)
    }

    /// Draws the menu (from `show`'s `FullOutput`) onto `view`, in the same
    /// encoder as the terminal grid.
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        screen_size: (u32, u32),
        pixels_per_point: f32,
        full_output: egui::FullOutput,
    ) {
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [screen_size.0, screen_size.1],
            pixels_per_point,
        };

        let clipped_primitives = self.ctx.tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, delta) in &full_output.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }

        let command_buffers =
            self.renderer.update_buffers(device, queue, encoder, &clipped_primitives, &screen_descriptor);
        if !command_buffers.is_empty() {
            queue.submit(command_buffers);
        }

        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &clipped_primitives, &screen_descriptor);
        }

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }
    }
}

/// Installs a native system sans-serif font (`render::system_ui_font_data`)
/// as the chrome's proportional font, ahead of egui's own bundled default —
/// so the context menu and settings panel read as this app's own native
/// chrome, not a generic toolkit's. A one-time swap at startup (unlike the
/// accent color, chrome typography isn't user-configurable, so there's
/// nothing to reapply on a settings change) — if the system font can't be
/// found, this silently leaves egui's own default in place rather than
/// erroring: a slightly-less-native font is a fine outcome, a blank/broken
/// one from a bad font load is not.
fn install_chrome_font(ctx: &egui::Context) {
    let Some((bytes, index)) = render::system_ui_font_data() else { return };
    let mut fonts = egui::FontDefinitions::default();
    let mut font_data = egui::FontData::from_owned(bytes.clone());
    font_data.index = *index;
    fonts.font_data.insert("system-sans".to_string(), std::sync::Arc::new(font_data));
    fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "system-sans".to_string());
    ctx.set_fonts(fonts);
}

/// Corner radius applied uniformly across all chrome — windows, menus, and
/// every widget state — instead of egui's stock 2-6px mix of roundedness.
/// A small, consistent radius reads as "technical" (matching the design
/// pass's brief); egui's stock default varies radius by widget/state, which
/// reads as generic toolkit chrome rather than a considered choice.
const RADIUS: u8 = 2;

// The "Graphite" palette (see project memory's design-pass entry), shared
// between `graphite_visuals` and `section_header` below. Matches
// `crates/app/src/graphics.rs`'s own Graphite constants exactly
// (`TITLE_BAR_BG`, `DIVIDER_COLOR`, `TEXT_COLOR`); `accent` is the only
// piece of this palette that isn't a fixed constant (Settings' "Accent
// color" instead), so it stays a `graphite_visuals` parameter.
const PANEL_BG: egui::Color32 = egui::Color32::from_rgb(0x14, 0x17, 0x1b); // graphics.rs's TITLE_BAR_BG
const FIELD_BG: egui::Color32 = egui::Color32::from_rgb(0x1b, 0x1f, 0x24); // one step up from PANEL_BG
const BORDER: egui::Color32 = egui::Color32::from_rgb(0x26, 0x2b, 0x31); // graphics.rs's DIVIDER_COLOR
const INK: egui::Color32 = egui::Color32::from_rgb(0xdf, 0xe2, 0xe6); // graphics.rs's TEXT_COLOR
const MUTED: egui::Color32 = egui::Color32::from_rgb(0x76, 0x7c, 0x85); // dimmed further from INK, for section headers/labels

/// The settings panel's `field-grid` proportions, taken from the mockup's
/// `grid-template-columns: 108px 1fr` — but as a *fraction* of the
/// window's own current width, not a hardcoded pixel count: at the
/// mockup's own ~420px window width, 108px is a ~26% label column, so
/// that's the ratio carried forward here. Pixel constants don't survive a
/// resize or a differently-scaled display; a fraction of "however wide the
/// window actually is right now" does, which is the whole reason the
/// Settings window is resizable again instead of pinned to one fixed
/// content width.
const LABEL_COL_FRACTION: f32 = 0.26;
/// The gap between the settings grid's two columns — also the `Grid`'s own
/// `spacing.x`, kept as one named constant so the column-width math above
/// and the `Grid::spacing` calls below can never silently drift apart.
const GRID_COLUMN_GAP: f32 = 12.0;

/// A small-caps-style section label (e.g. "BROADCAST", "APPEARANCE") for the
/// context menu and settings panel, matching the design pass's bordered-
/// section treatment: muted monospace, uppercased, letter-spaced, distinct
/// from the regular-weight body labels around it.
fn section_header(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text.to_uppercase()).monospace().size(9.5).color(MUTED).extra_letter_spacing(1.0));
    ui.add_space(4.0);
}

/// A right-aligned row of action buttons, explicitly bounded to a single
/// row's height.
///
/// `with_layout(right_to_left(..))` on its own claims all the remaining
/// vertical space of an auto-sizing window and centres the buttons within
/// it, which leaves a large dead gap above them and makes the window far
/// taller than its content — visible in a real screenshot of the paste
/// dialog. Allocating an explicit one-row region instead pins the height
/// to what the buttons actually need.
fn action_row(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::right_to_left(egui::Align::Center),
        add_contents,
    );
}

/// A settings-grid label pinned to `width`, instead of shrinking to fit
/// each label's own text — a plain `ui.label` left every `field-grid`'s
/// label column a different, narrower width (whatever its own longest row
/// happened to need), never matching the mockup's shared, wider column
/// across all three grids (Appearance/Terminal/Shell).
///
/// Not built on `add_sized`: that lays out its contents with
/// `Layout::centered_and_justified`, which centers text rather than
/// sitting it flush left the way the mockup's own `.field-grid label`
/// does. `with_main_justify(true)` reserves the same fixed-width cell
/// `add_sized` would (so the `Grid` column still measures exactly
/// `width`), while `with_main_align(Min)` keeps the text left-aligned
/// within it instead of centered.
fn grid_label(ui: &mut egui::Ui, text: &str, width: f32) {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.spacing().interact_size.y),
        egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Min).with_main_justify(true),
        |ui| ui.label(text),
    );
}

/// Wraps `add_contents` in a bordered, field-colored box at a fixed
/// `width` — for controls (the color swatches, the scrollback count) that
/// don't already draw their own such box the way `TextEdit` and
/// `ComboBox` do, matching the mockup's `.swatch-input` field style and
/// giving them the same "fills the column" presence as everything else in
/// the grid.
fn field_box(ui: &mut egui::Ui, width: f32, add_contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(FIELD_BG)
        .stroke(egui::Stroke::new(1.0, BORDER))
        .corner_radius(egui::CornerRadius::same(RADIUS))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.set_min_width((width - 20.0).max(40.0));
            add_contents(ui);
        });
}

fn color_field(ui: &mut egui::Ui, width: f32, rgb: &mut [f32; 3]) {
    field_box(ui, width, |ui| {
        ui.horizontal(|ui| {
            ui.color_edit_button_rgb(rgb);
            // Explicit `MUTED`, not the panel's usual ink: the mockup's
            // `swatch-input` hex text is deliberately dimmer than ordinary
            // body text, closer to a caption than a value.
            ui.label(egui::RichText::new(hex_rgb(*rgb)).monospace().color(MUTED));
        });
    });
}

/// Adds `slider` with its rail widened to fill `column_width`, instead of
/// egui's flat 100px style default (`Style::spacing.slider_width` — a
/// `Slider` has no per-instance width builder at all, confirmed in its own
/// source). Scoped locally via `ui.scope` so this doesn't leak into a
/// global style override that every slider in the app would need to share
/// one fixed number for, the same reasoning as everything else in this
/// pass: the width follows the column it's actually in, not a constant.
fn slider_field(ui: &mut egui::Ui, column_width: f32, slider: egui::Slider<'_>) {
    ui.scope(|ui| {
        ui.style_mut().spacing.slider_width = (column_width - 60.0).max(40.0);
        ui.add(slider);
    });
}

fn hex_rgb(rgb: [f32; 3]) -> String {
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", channel(rgb[0]), channel(rgb[1]), channel(rgb[2]))
}

/// Converts a 0.0–1.0 RGB triple (`config::Appearance`'s in-memory
/// convention) to egui's 0-255 `Color32` — shared between `graphite_visuals`
/// and any chrome code that needs the live accent color for an explicit
/// per-widget override (e.g. the broadcast radio row's active label).
fn color32_from_rgb(rgb: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb((rgb[0] * 255.0).round() as u8, (rgb[1] * 255.0).round() as u8, (rgb[2] * 255.0).round() as u8)
}

/// Shrinks egui's own default chrome text sizes toward the mockup's denser
/// "technical" type scale — its body text runs 11.5-12.5px against egui's
/// stock 13px, and its settings-panel title is a modest 12.5px bold label,
/// not egui's `TextStyle::Heading` (18px, dwarfing every section header
/// next to it) — plus tighter button padding/item spacing (the mockup's
/// buttons are 4px/8px padding and 5-7px gaps; egui's stock spacing reads
/// noticeably airier next to that). A one-time setup, like
/// `install_chrome_font` — none of this is user-configurable, so there's
/// nothing to reapply per frame.
fn apply_chrome_style(ctx: &egui::Context) {
    use egui::{FontId, TextStyle};
    ctx.all_styles_mut(|style| {
        style.text_styles.insert(TextStyle::Body, FontId::proportional(12.0));
        style.text_styles.insert(TextStyle::Button, FontId::proportional(12.0));
        style.text_styles.insert(TextStyle::Monospace, FontId::monospace(12.0));
        style.text_styles.insert(TextStyle::Heading, FontId::proportional(13.0));
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.item_spacing = egui::vec2(6.0, 6.0);
        // The settings-panel sliders override `slider_width` per instance
        // (see `slider_field`) to match their actual column width, but the
        // context menu has no such column to match — this is just a
        // saner app-wide fallback than egui's stock 100px in case a
        // slider ever shows up somewhere without an explicit override.
        style.spacing.slider_width = 160.0;
    });
}

/// The "Graphite" palette applied to egui's own chrome — context menu,
/// settings panel — so it matches the terminal grid's own colors instead of
/// egui's stock dark theme. `accent` is the one user-configurable piece
/// (Settings' "Accent color"); the rest is the fixed palette above.
fn graphite_visuals(accent_rgb: [f32; 3]) -> egui::Visuals {
    let accent = color32_from_rgb(accent_rgb);
    let panel_bg = PANEL_BG;
    let field_bg = FIELD_BG;
    let border = BORDER;
    let ink = INK;

    let mut visuals = egui::Visuals::dark();
    visuals.override_text_color = Some(ink);
    visuals.window_fill = panel_bg;
    visuals.panel_fill = panel_bg;
    visuals.faint_bg_color = field_bg;
    visuals.extreme_bg_color = field_bg;
    visuals.hyperlink_color = accent;
    visuals.selection.bg_fill = accent;
    visuals.selection.stroke.color = ink;
    visuals.window_stroke.color = border;

    let corner_radius = egui::CornerRadius::same(RADIUS);
    visuals.window_corner_radius = corner_radius;
    visuals.menu_corner_radius = corner_radius;

    visuals.widgets.noninteractive.bg_fill = panel_bg;
    visuals.widgets.noninteractive.weak_bg_fill = panel_bg;
    visuals.widgets.noninteractive.bg_stroke.color = border;
    visuals.widgets.noninteractive.fg_stroke.color = ink;
    visuals.widgets.noninteractive.corner_radius = corner_radius;

    visuals.widgets.inactive.bg_fill = field_bg;
    visuals.widgets.inactive.weak_bg_fill = field_bg;
    visuals.widgets.inactive.bg_stroke.color = border;
    visuals.widgets.inactive.fg_stroke.color = ink;
    visuals.widgets.inactive.corner_radius = corner_radius;

    visuals.widgets.hovered.bg_fill = field_bg;
    visuals.widgets.hovered.weak_bg_fill = field_bg;
    visuals.widgets.hovered.bg_stroke.color = accent;
    visuals.widgets.hovered.fg_stroke.color = accent;
    visuals.widgets.hovered.corner_radius = corner_radius;

    visuals.widgets.active.bg_fill = accent;
    visuals.widgets.active.weak_bg_fill = accent;
    visuals.widgets.active.bg_stroke.color = accent;
    visuals.widgets.active.fg_stroke.color = panel_bg;
    visuals.widgets.active.corner_radius = corner_radius;

    // `egui::Window`'s title bar is themed from this distinct widget state
    // (confirmed in `containers/window.rs`'s `title_ui`, which paints its
    // background from `widgets.open.weak_bg_fill` specifically) — every
    // other state above governs ordinary buttons/fields, not a window's
    // title bar, so leaving this one unset meant the Settings window kept
    // egui's stock near-white title bar (`Color32::from_gray(220)`) despite
    // every other part of the panel already matching Graphite.
    visuals.widgets.open.bg_fill = panel_bg;
    visuals.widgets.open.weak_bg_fill = panel_bg;
    visuals.widgets.open.bg_stroke.color = border;
    visuals.widgets.open.fg_stroke.color = ink;
    visuals.widgets.open.corner_radius = corner_radius;

    // Without this, a `Slider` paints only a bare rail plus a small
    // handle — no indication of progress at all, which is why it read as
    // "not even a slider" against the mockup's filled, accent-colored
    // track. `slider_trailing_fill` draws that fill using
    // `selection.bg_fill` (set to `accent` above), matching the mockup's
    // `.slider .fill` exactly, with no need to touch each `Slider` call
    // site individually.
    visuals.slider_trailing_fill = true;

    visuals
}
