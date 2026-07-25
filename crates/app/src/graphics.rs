//! GPU context for a window: surface, device, queue, and every pane's grid,
//! arranged per the layout tree.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::Receiver;

use layout::{Direction, Layout, Orientation, PaneId, Rect, SplitId};
use notify::Watcher;
use winit::window::Window;

use crate::color;
use crate::pane_session::PaneSession;
use crate::platform;

const DIVIDER_THICKNESS: f32 = 4.0;
/// Extra hit-test padding on each side of a divider's visual thickness — a
/// 4px line is a very thin target to grab with a mouse.
const DIVIDER_HIT_MARGIN: f32 = 4.0;
/// "Graphite" palette (see the design-pass memory entry): a cool near-
/// black ground with desaturated slate-gray chrome. `#262b31`.
const DIVIDER_COLOR: [f32; 4] = [0.149, 0.169, 0.192, 1.0];
/// `#dfe2e6` — Graphite's ink color, used for cell text and title-bar text
/// alike (an ungrouped pane's title bar is dark enough that the same
/// light ink reads fine there too; a grouped pane's random background
/// needs `contrasting_text_color` instead, since it might not be).
const TEXT_COLOR: [f32; 4] = [0.875, 0.886, 0.902, 1.0];

/// Drops `TEXT_COLOR`'s alpha channel, for use as a per-cell default color
/// (`color::resolve`'s API works in bare RGB — alpha is always opaque for
/// grid text, so there's nothing meaningful for a fourth channel to say).
fn rgb3(c: [f32; 4]) -> [f32; 3] {
    [c[0], c[1], c[2]]
}

/// Scales `settings.appearance.font_size` (a user-facing "points" value —
/// what's saved to config/session and shown in the Settings slider) by
/// the window's current DPI scale factor, so a given number renders at a
/// consistent physical size regardless of display scaling instead of
/// being interpreted as a literal physical-pixel count. Confirmed via a
/// real test machine at 125% Windows scaling: font size 13 looked correct
/// everywhere else but small in this app specifically, because nothing
/// here ever multiplied by the scale factor — unlike the egui chrome,
/// which already does this at its own boundary (see `Ui::show`'s
/// `pixels_per_point` conversion). Every call site that measures cell
/// size or rasterizes glyphs must go through this — `self.cell` (layout/
/// PTY sizing) and the actual rendered glyph size drifting apart, even
/// slightly, is exactly the class of bug Milestone 1's very first fix
/// (the hardcoded `CELL_WIDTH` glyph-bleed issue) was about.
fn scaled_font_size(font_size: f32, scale_factor: f64) -> f32 {
    font_size * scale_factor as f32
}

/// Fixed regardless of the user's chosen accent color — this is a
/// semantic signal (which panes are currently receiving broadcast input),
/// not a decorative/interactive highlight, so it stays put even if the
/// accent changes.
const BROADCAST_BORDER_COLOR: [f32; 4] = [0.95, 0.6, 0.15, 1.0];
const BROADCAST_BORDER_THICKNESS: f32 = 3.0;
/// Ratio delta applied per keyboard resize chord press.
const RESIZE_STEP: f32 = 0.03;

/// Padding above/below the title text within its bar, and to the left of
/// the group-name label.
const TITLE_BAR_PADDING: f32 = 4.0;
/// The title bar's close button — drawn as an ordinary glyph (the same
/// monospace grid every other title-bar character uses) rather than a
/// separate icon-rendering path, since a real "×" is already legible at
/// terminal font sizes and needs nothing beyond what glyph rasterization
/// already does for the title text right next to it.
const CLOSE_BUTTON_GLYPH: char = '×';
/// Default (ungrouped) title bar colors — Graphite's own dark surface
/// tone (`#14171b`) and ink, fixed regardless of luminance (unlike
/// grouped panes, whose random background needs a computed contrast color
/// instead).
const TITLE_BAR_BG: [f32; 4] = [0.078, 0.090, 0.106, 1.0];
const TITLE_BAR_TEXT_LIGHT: [f32; 4] = TEXT_COLOR;
/// Graphite's own ground tone, reused here as the "dark ink" choice for a
/// bright grouped pane's title bar — the same hue family as the rest of
/// the chrome rather than a plain neutral black.
const TITLE_BAR_TEXT_DARK: [f32; 4] = [0.047, 0.055, 0.067, 1.0];
/// A grouped pane's title bar background is picked from this set, keyed by
/// a hash of the group's name (stable across reloads/restarts — the same
/// group name always gets the same color, rather than actually re-rolling
/// randomly each time, which would make a group's identity flicker on
/// every rename/reassignment round-trip). Chosen for roughly even hue
/// spacing at a medium, "reasonably visible" saturation/lightness that
/// works against either a light or dark text overlay.
const GROUP_COLOR_PALETTE: [[f32; 4]; 10] = [
    [0.78, 0.25, 0.25, 1.0], // red
    [0.82, 0.47, 0.16, 1.0], // orange
    [0.80, 0.70, 0.20, 1.0], // amber
    [0.45, 0.65, 0.25, 1.0], // green
    [0.20, 0.60, 0.55, 1.0], // teal
    [0.25, 0.55, 0.80, 1.0], // blue
    [0.40, 0.42, 0.80, 1.0], // indigo
    [0.60, 0.35, 0.80, 1.0], // purple
    [0.80, 0.35, 0.65, 1.0], // magenta
    [0.55, 0.75, 0.25, 1.0], // lime
];

pub struct Graphics {
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    grid: render::GridRenderer,
    cell: (f32, f32),
    layout: Layout,
    panes: HashMap<PaneId, PaneSession>,
    focused: PaneId,
    router: router::Router,
    /// The divider currently being dragged, if any: its id, orientation
    /// (which axis a drag delta applies to), and the pixel length of the
    /// parent area it splits (to convert a pixel delta into a ratio delta).
    dragging: Option<(SplitId, Orientation, f32)>,
    /// The pane and button a mouse-reporting gesture is forwarding to, from
    /// press to release — kept distinct from `dragging` since a press
    /// starts at most one of the two, never both (a divider isn't inside
    /// either pane it separates).
    mouse_gesture: Option<(PaneId, crate::mouse::Button)>,
    /// The pane an in-grid text-selection drag is in progress for, if any —
    /// the "otherwise" arm of the same press `mouse_gesture` handles: a
    /// pane whose program hasn't turned on mouse reporting gets a local
    /// selection instead of a forwarded click.
    selecting: Option<PaneId>,
    ui: crate::ui::Ui,
    /// The user's config — loaded once here for now; Milestone 5.2 (hot
    /// reload) replaces this wholesale on a valid re-parse, and 5.3/5.4
    /// (keybinding overrides, settings panel) read/write the same struct
    /// rather than keeping a separate copy, per
    /// `.waypoint/design/config-system.md`. Named `settings`, not `config`,
    /// to stay distinct from the `wgpu::SurfaceConfiguration` field already
    /// using that name.
    settings: config::Config,
    /// The last durably-saved config — distinct from `settings`, which now
    /// also reflects the settings panel's *in-progress, unsaved* edits
    /// live (see `redraw`'s live-preview step): this is what Cancel (or
    /// closing the panel via its own close button, without Save) reverts
    /// `settings` back to.
    saved_settings: config::Config,
    /// Fires (payload discarded — a reload just re-reads the whole file)
    /// whenever the config directory changes, from a background thread
    /// `notify` runs its own watcher on. `None` if the watcher couldn't be
    /// started (e.g. the config directory isn't creatable) — hot reload is
    /// best-effort, never a reason to fail startup.
    config_reload_rx: Option<Receiver<()>>,
    /// Kept alive only so `notify`'s background watch thread keeps running
    /// — never read after construction, but dropping it stops the watch.
    _config_watcher: Option<notify::RecommendedWatcher>,
    /// Shared, throttled process-list snapshot every pane's title bar reads
    /// from — see `crate::foreground_process`.
    foreground_processes: crate::foreground_process::ForegroundProcesses,
}

impl Graphics {
    /// Initializes a wgpu surface, adapter, and device targeting `window`.
    /// With `session`, rebuilds its saved layout, panes (spawned into their
    /// saved cwds — never restarting whatever was running, CONOPS §5g), and
    /// group membership; `None` spawns a single shell into one pane filling
    /// the window, same as always.
    pub fn new(window: Arc<Window>, session: Option<session::Session>) -> anyhow::Result<Self> {
        let size = window.inner_size();
        // On Windows, a plain HWND-backed swapchain (DX12 or Vulkan alike)
        // only ever reports `CompositeAlphaMode::Opaque` — real per-pixel
        // window transparency there needs a DirectComposition-backed
        // swapchain instead, which is only implemented for wgpu-hal's DX12
        // backend (`Dx12SwapchainKind::DxgiFromVisual`, confirmed by
        // reading `wgpu-hal`'s dx12 backend source: it lazily creates its
        // own `IDCompositionDevice`/`Target`/`Visual` for the window handle
        // internally — nothing else in this app needs to touch DirectComposition
        // directly). Forcing the backend to DX12 here, rather than leaving
        // the default "try every backend" selection, guarantees that path
        // is actually used instead of possibly landing on Vulkan (whose
        // Windows WSI has the same Opaque-only limitation as a plain DX12
        // HWND surface, with no composition-visual escape hatch).
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: platform_backends(),
            backend_options: wgpu::BackendOptions {
                dx12: wgpu::Dx12BackendOptions {
                    presentation_system: wgpu::Dx12SwapchainKind::DxgiFromVisual,
                    ..Default::default()
                },
                ..Default::default()
            },
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(window.clone())?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            compatible_surface: Some(&surface),
            ..Default::default()
        }))?;

        if crate::verbose::is_verbose(crate::verbose::Category::General) {
            let info = adapter.get_info();
            eprintln!(
                "wgpu: {} ({:?} backend, {:?}, driver: {} {})",
                info.name, info.backend, info.device_type, info.driver, info.driver_info
            );
        }

        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;

        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or_else(|| anyhow::anyhow!("adapter does not support this surface"))?;
        // `PreMultiplied`: the compositor expects our stored color to
        // already be RGB×alpha (`render`'s pipeline produces exactly that —
        // see its `PREMULTIPLIED_ALPHA_BLENDING` blend state and `fs_main`'s
        // own comment). This isn't a free choice: on Windows, a
        // DirectComposition swapchain (needed for transparency there at
        // all) rejects `STRAIGHT`/`PostMultiplied` outright — confirmed via
        // the D3D12 debug layer ("Composition SwapChains do not support the
        // DXGI_ALPHA_MODE_STRAIGHT AlphaMode") — so `PreMultiplied` is the
        // only mode that actually works there, and the renderer was changed
        // to match it everywhere rather than branching per-platform.
        // `get_default_config`'s own `Auto` choice doesn't reliably pick a
        // mode that honors alpha at all — on many platforms it resolves to
        // `Opaque`, which is what "changing the transparency slider did
        // nothing" would look like if this weren't selected explicitly.
        // Falls back to whatever `Auto` gives (typically `Opaque`) when
        // `PreMultiplied` isn't offered — transparency just has no visible
        // effect there, logged once rather than treated as an error.
        //
        // WSL is excluded outright, not just left to fall back naturally:
        // WSLg's compositor doesn't handle this correctly even though it
        // does report `PreMultiplied` as available — observed as the whole
        // window going fully see-through regardless of the configured
        // level, plus mouse clicks passing through it. WSL isn't a target
        // platform here (Windows and native Linux are), so this is treated
        // the same as the WSLg cursor-theme quirks already documented in
        // project memory: not chased, just not attempted.
        let caps = surface.get_capabilities(&adapter);
        if !platform::is_wsl() && caps.alpha_modes.contains(&wgpu::CompositeAlphaMode::PreMultiplied) {
            config.alpha_mode = wgpu::CompositeAlphaMode::PreMultiplied;
        } else if crate::verbose::is_verbose(crate::verbose::Category::General) {
            eprintln!(
                "wgpu: transparency unavailable here (offers {:?}, wsl={}); window transparency will have no visible effect",
                caps.alpha_modes,
                platform::is_wsl()
            );
        }
        surface.configure(&device, &config);

        let config_path = config::Config::default_path();
        let settings = config::Config::load(&config_path);
        let saved_settings = settings.clone();
        if crate::verbose::is_verbose(crate::verbose::Category::General) {
            eprintln!("config: loaded {settings:?}");
        }
        let (_config_watcher, config_reload_rx) = match watch_config_dir(&config_path) {
            Some((watcher, rx)) => (Some(watcher), Some(rx)),
            None => (None, None),
        };

        let grid = render::GridRenderer::new(&device, &queue, config.format);
        let ui = crate::ui::Ui::new(&device, config.format, &window);
        let cell = render::measure_cell(
            scaled_font_size(settings.appearance.font_size, window.scale_factor()),
            &settings.appearance.font_family,
        );

        // A restored session's per-pane state (cwd, group), matched
        // positionally against `panes_order` — both walk the tree in the
        // same left-to-right, depth-first order (see `layout::SavedNode`'s
        // doc comment). A pane-count mismatch means a corrupted or
        // otherwise unusable file; treated the same as no session at all
        // rather than restoring a partial/misaligned guess.
        let (layout, panes_order, pane_states): (Layout, Vec<PaneId>, Vec<Option<session::PaneState>>) =
            match session.and_then(|s| {
                let (layout, order) = Layout::from_snapshot(&s.layout);
                (order.len() == s.panes.len()).then_some((layout, order, s.panes))
            }) {
                Some((layout, order, states)) => (layout, order, states.into_iter().map(Some).collect()),
                None => {
                    let (layout, root) = Layout::new();
                    (layout, vec![root], vec![None])
                }
            };

        // A placeholder — corrected immediately below by
        // `resize_panes_to_geometry` once every pane (and so the real
        // layout geometry) exists, exactly as a window resize would. Only
        // exactly right for a single pane filling the whole window (the
        // no-session-restored case), but spawning any pane briefly at the
        // wrong size before its very first paint is harmless, the same as
        // an ordinary resize.
        let root_size = Self::rect_to_size(
            Self::content_rect(
                Rect { x: 0.0, y: 0.0, width: size.width as f32, height: size.height as f32 },
                cell,
            ),
            cell,
        );
        if crate::verbose::is_verbose(crate::verbose::Category::General) {
            eprintln!(
                "pane: spawning {} pane(s) at up to {}x{} cells (window {}x{}px, cell {}x{}px)",
                panes_order.len(),
                root_size.cols,
                root_size.rows,
                size.width,
                size.height,
                cell.0,
                cell.1
            );
        }

        let mut router = router::Router::new();
        router.keymap.apply_overrides(&settings.keybindings);

        let mut panes = HashMap::new();
        for (pane_id, state) in panes_order.iter().zip(&pane_states) {
            let cwd = state.as_ref().map(|s| s.cwd.as_path());
            // A saved explicit shell (e.g. a past "Swap shell") wins;
            // otherwise fall back to whatever the *current* configured
            // default is, same as a pane that's never been touched.
            let shell = state.as_ref().and_then(|s| s.shell.as_deref()).or_else(|| Self::shell(&settings));
            match PaneSession::spawn(shell, root_size, cwd) {
                Ok(session) => {
                    panes.insert(*pane_id, session);
                    if let Some(group) = state.as_ref().and_then(|s| s.group.clone()) {
                        router.assign_to_group(*pane_id, group);
                    }
                }
                Err(err) => eprintln!("pane: failed to spawn: {err:#}"),
            }
        }
        if panes.is_empty() {
            anyhow::bail!("failed to spawn any pane");
        }
        // The tree's first pane, restored or not — session restore
        // doesn't track which pane had focus (not part of what CONOPS §5g
        // asks this to persist), so this is as reasonable a default as any.
        let focused = panes_order.into_iter().find(|p| panes.contains_key(p)).expect("checked non-empty above");

        let mut graphics = Self {
            window,
            surface,
            device,
            queue,
            config,
            grid,
            cell,
            layout,
            panes,
            focused,
            router,
            dragging: None,
            mouse_gesture: None,
            selecting: None,
            ui,
            settings,
            saved_settings,
            config_reload_rx,
            _config_watcher,
            foreground_processes: crate::foreground_process::ForegroundProcesses::new(),
        };
        graphics.resize_panes_to_geometry();
        Ok(graphics)
    }

    /// Assembles the current window size, layout, and every pane's cwd/
    /// group/shell into a `session::Session` and writes it out — called
    /// from every quit path (`main.rs`). Does nothing if there are no panes (an
    /// empty session isn't meaningful; the next launch just starts fresh
    /// the normal way) or if the write itself fails (logged, never a
    /// reason to block quitting).
    pub fn save_session(&mut self) {
        let pane_order = self.layout.panes();
        if pane_order.is_empty() {
            return;
        }

        let mut pane_states = Vec::with_capacity(pane_order.len());
        for pane in &pane_order {
            let Some(pane_session) = self.panes.get(pane) else { continue };
            let cwd = pane_session.cwd(&mut self.foreground_processes);
            let group = self.router.group_of(*pane).map(|g| g.0);
            let shell = pane_session.shell().map(str::to_string);
            pane_states.push(session::PaneState { cwd, group, shell });
        }

        let to_save = session::Session {
            window: session::WindowSize { width: self.config.width, height: self.config.height },
            layout: self.layout.snapshot(),
            panes: pane_states,
        };
        if let Err(err) = to_save.save(&session::Session::default_path()) {
            eprintln!("session: failed to save: {err:#}");
        }
    }

    /// The configured default shell, or `None` to let `portable-pty` pick
    /// the platform default — an empty string in config means the latter,
    /// per `.waypoint/design/config-system.md`.
    fn shell(settings: &config::Config) -> Option<&str> {
        (!settings.general.default_shell.is_empty()).then_some(settings.general.default_shell.as_str())
    }

    /// Re-reads the config file if the watcher (if any) has reported a
    /// change since the last call, applying it to live state on success.
    /// A bad edit is reported to stderr and otherwise ignored — whatever
    /// was running keeps running, per `.waypoint/design/config-system.md`'s
    /// "never crash or blank the session" rule. Called once per frame from
    /// `redraw`, same as pane-exit polling.
    fn poll_config_reload(&mut self) {
        let Some(rx) = &self.config_reload_rx else { return };
        // Drain every pending notification — a single edit can fire more
        // than one (some editors save via a temp-file-plus-rename, which
        // is two filesystem events for one logical change) — and react
        // once, re-reading current file contents rather than anything
        // carried in the event itself.
        let mut changed = false;
        while rx.try_recv().is_ok() {
            changed = true;
        }
        if !changed {
            return;
        }

        match config::Config::try_load(&config::Config::default_path()) {
            Ok(new_settings) if new_settings == self.settings => {}
            Ok(new_settings) => self.apply_settings(new_settings),
            Err(err) => {
                eprintln!("config: edit not applied, keeping previous settings: {err}");
            }
        }
    }

    /// Applies a freshly (re-)loaded config to live state: anything whose
    /// effect is just read fresh off `self.settings` each frame needs
    /// nothing further, but font size feeds into cell measurement and
    /// every pane's PTY/grid size, so a change there has to trigger the
    /// same resize path a window resize or split would.
    fn apply_settings(&mut self, new_settings: config::Config) {
        let font_size_changed = new_settings.appearance.font_size != self.settings.appearance.font_size;
        let font_family_changed = new_settings.appearance.font_family != self.settings.appearance.font_family;
        let keybindings_changed = new_settings.keybindings != self.settings.keybindings;
        if crate::verbose::is_verbose(crate::verbose::Category::General) {
            eprintln!("config: reloaded {new_settings:?}");
        }
        self.settings = new_settings;
        if font_size_changed || font_family_changed {
            self.cell = render::measure_cell(
                scaled_font_size(self.settings.appearance.font_size, self.window.scale_factor()),
                &self.settings.appearance.font_family,
            );
            self.resize_panes_to_geometry();
        }
        if keybindings_changed {
            // Rebuilt from scratch each time, not patched incrementally, so
            // a since-removed override reverts its chord to the built-in
            // default instead of staying stuck at a stale rebinding.
            self.router.keymap = router::Keymap::terminator_defaults();
            self.router.keymap.apply_overrides(&self.settings.keybindings);
        }
    }

    /// Feeds a window event to the UI overlay. Returns whether it was
    /// consumed — the caller should skip pane/divider handling of the same
    /// event when this is true.
    pub fn ui_consume_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        self.ui.on_window_event(&self.window, event)
    }

    /// Opens the pane-management context menu (Broadcast/Split/Arrange/
    /// Group/Swap shell/Settings) for whichever pane's *title bar* is under
    /// `pos`, if any. Returns whether one opened — the caller falls back to
    /// `open_terminal_context_menu_at` when it didn't, so a right-click
    /// anywhere else in the pane gets the terminal (copy/paste) menu
    /// instead.
    pub fn open_context_menu_at(&mut self, pos: (f32, f32)) -> bool {
        let Some(pane) = self.pane_title_bar_at(pos) else { return false };
        self.ui.open_context_menu(pane, pos);
        true
    }

    /// Opens the terminal (copy/paste) context menu for whichever pane is
    /// under `pos`, if any — for a right-click that landed on the terminal
    /// content itself, not a title bar (see `open_context_menu_at`).
    pub fn open_terminal_context_menu_at(&mut self, pos: (f32, f32)) {
        if let Some(pane) = self.pane_at(pos) {
            self.ui.open_terminal_context_menu(pane, pos);
        }
    }

    /// Closes whichever context menu is open. Returns whether one was.
    pub fn close_context_menu(&mut self) -> bool {
        self.ui.close_context_menu()
    }

    /// Whether the UI overlay currently has a menu or the settings panel
    /// open — see `Ui::wants_keyboard_focus` and `main.rs`'s Tab-key
    /// handling.
    pub fn ui_wants_keyboard_focus(&self) -> bool {
        self.ui.wants_keyboard_focus()
    }

    /// The window this GPU context is rendering into.
    pub fn window(&self) -> &Window {
        &self.window
    }

    fn area(&self) -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: self.config.width as f32,
            height: self.config.height as f32,
        }
    }

    fn rect_to_size(rect: Rect, cell: (f32, f32)) -> pane::Size {
        pane::Size {
            rows: ((rect.height / cell.1) as u16).max(1),
            cols: ((rect.width / cell.0) as u16).max(1),
        }
    }

    /// Height of a pane's title bar, scaled to the current font size so the
    /// centered/left-aligned labels always have room to sit comfortably —
    /// a fixed pixel constant would either waste space at large font sizes
    /// or clip text at small ones.
    fn title_bar_height(cell: (f32, f32)) -> f32 {
        cell.1 + TITLE_BAR_PADDING * 2.0
    }

    /// A pane's rect with its title bar carved off the top — the actual
    /// terminal grid (rows/cols sizing, cursor/selection/text positioning)
    /// only ever occupies this, never the full pane rect; the title bar
    /// itself is chrome drawn separately in `redraw`.
    fn content_rect(rect: Rect, cell: (f32, f32)) -> Rect {
        let title_bar = Self::title_bar_height(cell);
        Rect {
            x: rect.x,
            y: rect.y + title_bar,
            width: rect.width,
            height: (rect.height - title_bar).max(0.0),
        }
    }

    /// The clickable close-button rect within a pane's title bar — a
    /// *square* of `TITLE_BAR_PADDING` from the title bar's top, right,
    /// and bottom edges alike. Deliberately not `cell.0` (glyph advance
    /// width) by `cell.1` (line height): those two are wildly different
    /// magnitudes for a typical monospace font (line height usually
    /// runs 2x+ a glyph's advance width), so reusing them directly gave
    /// the button a tall, narrow shape — the same fixed padding value
    /// looked "uniform" only in raw pixels, not in how balanced the
    /// button itself actually read next to a symbol centered inside it.
    /// Shared between drawing (`redraw`) and hit-testing
    /// (`close_button_at`) so they can never silently drift apart.
    fn close_button_rect(full: Rect, cell: (f32, f32)) -> Rect {
        let side = cell.1;
        Rect {
            x: full.x + full.width - TITLE_BAR_PADDING - side,
            y: full.y + TITLE_BAR_PADDING,
            width: side,
            height: side,
        }
    }

    /// Resizes every currently-visible pane's PTY and grid to match the
    /// layout's current geometry. Called after anything that changes pane
    /// rects: window resize, split, close, zoom toggle, divider drag.
    fn resize_panes_to_geometry(&mut self) {
        let geometry = self.layout.geometry(self.area(), DIVIDER_THICKNESS);
        for pane_rect in &geometry.panes {
            if let Some(session) = self.panes.get_mut(&pane_rect.pane) {
                let size = Self::rect_to_size(Self::content_rect(pane_rect.rect, self.cell), self.cell);
                if let Err(err) = session.resize(size) {
                    eprintln!("pane: failed to resize: {err:#}");
                }
            }
        }
    }

    /// Reconfigures the surface for a new window size and resizes every
    /// visible pane to match.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.resize_panes_to_geometry();
    }

    /// Recomputes cell size for the window's current DPI scale factor —
    /// call on `WindowEvent::ScaleFactorChanged` (the window was dragged
    /// to a monitor with a different scaling setting, or the OS-level
    /// scale changed). Font size is stored as a user-facing "points"
    /// value and scaled by the OS setting at measurement/render time (see
    /// `scaled_font_size`), not baked into `self.cell` permanently, so
    /// it has to be recomputed whenever that scale factor itself
    /// changes — the same as a font-size settings change.
    pub fn rescale(&mut self) {
        self.cell = render::measure_cell(
            scaled_font_size(self.settings.appearance.font_size, self.window.scale_factor()),
            &self.settings.appearance.font_family,
        );
        self.resize_panes_to_geometry();
    }

    /// Forwards keyboard input to the focused pane's shell, or to every
    /// pane in the current broadcast target set (see
    /// `.waypoint/design/input-router.md`).
    pub fn send_input(&mut self, data: &[u8]) -> anyhow::Result<()> {
        let all_panes = self.layout.panes();
        let targets = self.router.broadcast_targets(self.focused, &all_panes);
        for pane in targets {
            if let Some(session) = self.panes.get_mut(&pane) {
                session.write_input(data)?;
            }
        }
        Ok(())
    }

    /// Resolves `chord` via the keymap and, if bound, executes the action.
    /// Returns `None` if the chord isn't bound — the caller should treat
    /// the key as passthrough input instead, since a chord is never
    /// partially consumed. `Some(false)` means the app should quit.
    pub fn dispatch_chord(&mut self, chord: router::Chord) -> Option<bool> {
        let action = self.router.resolve(chord)?;
        Some(match action {
            router::Action::Split(orientation) => {
                self.split(orientation);
                true
            }
            router::Action::ClosePane => self.close_focused(),
            router::Action::Quit => false,
            router::Action::Focus(direction) => {
                self.focus(direction);
                true
            }
            router::Action::Resize(direction) => {
                self.resize_focused(direction);
                true
            }
            router::Action::ToggleZoom => {
                self.toggle_zoom();
                true
            }
            router::Action::SetBroadcastMode(mode) => {
                self.router.broadcast_mode = mode;
                true
            }
        })
    }

    /// Splits the focused pane, spawning a fresh shell into the new pane and
    /// focusing it. The keyboard-chord entry point — chords inherently act
    /// on "whatever's focused," unlike the context menu, which targets
    /// whichever pane was right-clicked (see `split_pane`).
    pub fn split(&mut self, orientation: Orientation) {
        self.split_pane(self.focused, orientation);
    }

    /// Splits `pane` specifically, spawning a fresh shell into the new pane
    /// and focusing it. No-op if `pane` no longer exists (e.g. a context-
    /// menu split request arriving after that pane already closed).
    pub fn split_pane(&mut self, pane: PaneId, orientation: Orientation) {
        let Some(new_pane) = self.layout.split(pane, orientation) else {
            return;
        };
        self.resize_panes_to_geometry();

        let geometry = self.layout.geometry(self.area(), DIVIDER_THICKNESS);
        let rect = geometry
            .panes
            .iter()
            .find(|p| p.pane == new_pane)
            .expect("freshly split pane must appear in geometry")
            .rect;
        let size = Self::rect_to_size(Self::content_rect(rect, self.cell), self.cell);

        match PaneSession::spawn(Self::shell(&self.settings), size, None) {
            Ok(session) => {
                self.panes.insert(new_pane, session);
                self.focused = new_pane;
            }
            Err(err) => {
                eprintln!("pane: failed to spawn split: {err:#}");
                self.layout.close(new_pane);
            }
        }
    }

    /// Kills `pane`'s current shell and starts a fresh one in its place,
    /// leaving the pane's position, size, group membership, and broadcast
    /// participation untouched — unlike `close_pane`, which tears all of
    /// that down too. `shell` follows the same `None`-means-platform-
    /// default convention as `Self::shell`. No-op if `pane` no longer
    /// exists (e.g. a context-menu request arriving after that pane already
    /// closed).
    ///
    /// For cases the context menu's "Swap shell" item exists to cover: a
    /// pane's foreground-process detection can only see as far as the
    /// process tree/pgid the pane's own OS knows about (`foreground_process`
    /// module docs) — running e.g. `wsl.exe` from inside a Windows shell
    /// crosses into a different kernel's process list entirely, which is
    /// invisible from the Windows side, so the title bar gets stuck showing
    /// `wsl.exe` no matter what runs inside it. There's no detection fix for
    /// that; swapping the pane directly into the nested shell sidesteps the
    /// boundary instead.
    pub fn restart_pane_shell(&mut self, pane: PaneId, shell: Option<&str>) {
        if !self.panes.contains_key(&pane) {
            return;
        }
        let geometry = self.layout.geometry(self.area(), DIVIDER_THICKNESS);
        let Some(rect) = geometry.panes.iter().find(|p| p.pane == pane).map(|p| p.rect) else {
            return;
        };
        let size = Self::rect_to_size(Self::content_rect(rect, self.cell), self.cell);

        match PaneSession::spawn(shell, size, None) {
            Ok(session) => {
                // Replacing the map entry drops the old `PaneSession`,
                // whose `Pty::drop` kills the old shell — no explicit kill
                // call needed.
                self.panes.insert(pane, session);
            }
            Err(err) => eprintln!("pane: failed to restart shell: {err:#}"),
        }
    }

    /// Rearranges every pane currently open into a preset shape (see
    /// `layout::Arrangement`) — from the context menu's "Arrange all
    /// panes" section. Every existing `PaneSession` is kept exactly as it
    /// is (no shells respawned, nothing torn down); only each pane's
    /// position and size change. Group membership and broadcast state
    /// (both keyed by `PaneId`, none of which change here) carry over
    /// automatically, and `self.focused` stays valid without needing an
    /// update — rearranging never removes a pane.
    pub fn arrange_panes(&mut self, arrangement: layout::Arrangement) {
        let panes = self.layout.panes();
        self.layout = Layout::arrange(&panes, arrangement);
        self.resize_panes_to_geometry();
    }

    /// Closes `pane` — used for an explicit close action (the title-bar
    /// close button, a right-click menu's "Close", or the
    /// `Ctrl+Shift+W`/`close_focused` chord) and for a pane whose shell
    /// has exited on its own. Returns `false` if it was the last pane in
    /// the layout — the caller should treat that as "quit", since the
    /// tree can't express closing its own last leaf.
    pub fn close_pane(&mut self, pane: PaneId) -> bool {
        let closed = self.layout.close(pane);
        if closed {
            self.panes.remove(&pane);
            self.router.forget_pane(pane);
            // `PaneId`s are assigned by an ever-incrementing counter and
            // never reused, so the highest surviving id is also the most
            // recently created pane — exactly what should get focus next.
            if self.focused == pane
                && let Some(next) = self.layout.panes().iter().max().copied()
            {
                self.focused = next;
            }
            self.resize_panes_to_geometry();
        }
        closed
    }

    /// Closes the focused pane. Returns `false` if it was the last pane in
    /// the layout — the caller should treat that as "quit".
    pub fn close_focused(&mut self) -> bool {
        self.close_pane(self.focused)
    }

    /// Moves focus to the pane adjacent to the current one in `direction`,
    /// if there is one.
    pub fn focus(&mut self, direction: Direction) {
        if let Some(next) = self.layout.focus_neighbor(self.focused, direction, self.area()) {
            self.focused = next;
        }
    }

    /// Toggles zoom on the focused pane.
    pub fn toggle_zoom(&mut self) {
        self.layout.toggle_zoom(self.focused);
        self.resize_panes_to_geometry();
    }

    /// Scrolls whichever pane is under `pos` (window pixel coordinates) by
    /// the wheel movement `delta` represents. `LineDelta` (a physical wheel
    /// with discrete notches, the common case) maps one notch to one line;
    /// `PixelDelta` (precision trackpads) converts through the pane's own
    /// row height, so a scroll gesture covers a consistent visual distance
    /// regardless of input device. Returns whether a pane was actually
    /// found under `pos` (and so needs a redraw) — nothing under the
    /// cursor means nothing to scroll.
    pub fn scroll_at(&mut self, pos: (f32, f32), delta: winit::event::MouseScrollDelta) -> bool {
        let lines = match delta {
            winit::event::MouseScrollDelta::LineDelta(_, y) => y.round() as i32,
            winit::event::MouseScrollDelta::PixelDelta(px) => (px.y as f32 / self.cell.1).round() as i32,
        };
        if lines == 0 {
            return false;
        }
        let Some(pane) = self.pane_at(pos) else { return false };
        let Some(session) = self.panes.get_mut(&pane) else { return false };
        session.scroll(lines);
        true
    }

    /// Focuses whichever pane is under `pos` (window pixel coordinates), if
    /// any. Returns whether focus changed. A left click landing inside a
    /// pane should always focus it before anything else the click might do
    /// (start a selection, forward a click to the shell) — matching every
    /// other multi-pane terminal's click-to-focus convention.
    pub fn focus_at(&mut self, pos: (f32, f32)) -> bool {
        match self.pane_at(pos) {
            Some(pane) if pane != self.focused => {
                self.focused = pane;
                true
            }
            _ => false,
        }
    }

    /// The pane whose rect contains `pos` (window pixel coordinates), if
    /// any — for right-click context menu targeting.
    pub fn pane_at(&self, pos: (f32, f32)) -> Option<PaneId> {
        let geometry = self.layout.geometry(self.area(), DIVIDER_THICKNESS);
        geometry
            .panes
            .iter()
            .find(|p| {
                let rect = p.rect;
                pos.0 >= rect.x && pos.0 < rect.x + rect.width && pos.1 >= rect.y && pos.1 < rect.y + rect.height
            })
            .map(|p| p.pane)
    }

    /// The pane whose *title bar* rect (not its whole rect — see
    /// `content_rect`) contains `pos`, if any. Distinguishes a right-click
    /// on a pane's title bar (opens the pane-management menu) from one on
    /// its terminal content (opens the copy/paste menu instead).
    pub fn pane_title_bar_at(&self, pos: (f32, f32)) -> Option<PaneId> {
        let geometry = self.layout.geometry(self.area(), DIVIDER_THICKNESS);
        geometry
            .panes
            .iter()
            .find(|p| {
                let rect = p.rect;
                let title_bar_bottom = rect.y + Self::title_bar_height(self.cell);
                pos.0 >= rect.x && pos.0 < rect.x + rect.width && pos.1 >= rect.y && pos.1 < title_bar_bottom
            })
            .map(|p| p.pane)
    }

    /// The pane whose title-bar close button contains `pos`, if any — for
    /// left-click handling, checked before ordinary focus/divider-drag
    /// handling so a click landing on the close button always closes that
    /// pane instead of also being treated as a normal pane click.
    pub fn close_button_at(&self, pos: (f32, f32)) -> Option<PaneId> {
        let geometry = self.layout.geometry(self.area(), DIVIDER_THICKNESS);
        geometry
            .panes
            .iter()
            .find(|p| {
                let rect = Self::close_button_rect(p.rect, self.cell);
                pos.0 >= rect.x && pos.0 < rect.x + rect.width && pos.1 >= rect.y && pos.1 < rect.y + rect.height
            })
            .map(|p| p.pane)
    }

    /// Resizes the split adjacent to the focused pane along `direction`'s
    /// axis. `Right`/`Down` always grow the focused pane along that axis,
    /// `Left`/`Up` always shrink it, regardless of which side of the split
    /// it's on — the simplest convention that stays predictable across
    /// nested splits (see `layout::Layout::resize_target`'s doc comment).
    /// No-op if there's no ancestor split on that axis.
    pub fn resize_focused(&mut self, direction: Direction) {
        let Some((split, is_first)) = self.layout.resize_target(self.focused, direction) else {
            return;
        };
        let grows = matches!(direction, Direction::Right | Direction::Down);
        let delta = if grows == is_first { RESIZE_STEP } else { -RESIZE_STEP };
        self.layout.resize(split, delta);
        self.resize_panes_to_geometry();
    }

    /// Finds the divider under `pos` (window pixel coordinates), if any,
    /// padding its hit-test region by `DIVIDER_HIT_MARGIN` beyond its
    /// visual thickness since that thickness alone is too thin a target to
    /// grab reliably.
    fn divider_hit(&self, pos: (f32, f32)) -> Option<(SplitId, Orientation, f32)> {
        let geometry = self.layout.geometry(self.area(), DIVIDER_THICKNESS);
        geometry.dividers.iter().find_map(|d| {
            let rect = d.rect;
            let (min_x, max_x, min_y, max_y) = match d.orientation {
                Orientation::Horizontal => (
                    rect.x - DIVIDER_HIT_MARGIN,
                    rect.x + rect.width + DIVIDER_HIT_MARGIN,
                    rect.y,
                    rect.y + rect.height,
                ),
                Orientation::Vertical => (
                    rect.x,
                    rect.x + rect.width,
                    rect.y - DIVIDER_HIT_MARGIN,
                    rect.y + rect.height + DIVIDER_HIT_MARGIN,
                ),
            };
            (pos.0 >= min_x && pos.0 < max_x && pos.1 >= min_y && pos.1 < max_y)
                .then_some((d.split, d.orientation, d.axis_extent))
        })
    }

    /// The orientation of the divider under `pos`, if any — for choosing a
    /// hover cursor icon.
    pub fn divider_orientation_at(&self, pos: (f32, f32)) -> Option<Orientation> {
        self.divider_hit(pos).map(|(_, orientation, _)| orientation)
    }

    /// Hit-tests `pos` (window pixel coordinates) against divider rects and
    /// begins a drag if one is hit. Returns whether a drag started.
    pub fn begin_drag(&mut self, pos: (f32, f32)) -> bool {
        if crate::verbose::is_verbose(crate::verbose::Category::Mouse) {
            eprintln!(
                "mouse: begin_drag at {pos:?}, hit={:?}",
                self.divider_hit(pos).map(|(_, o, _)| o)
            );
        }
        match self.divider_hit(pos) {
            Some(hit) => {
                self.dragging = Some(hit);
                true
            }
            None => false,
        }
    }

    /// Whether a divider drag is in progress.
    pub fn is_dragging(&self) -> bool {
        self.dragging.is_some()
    }

    /// Continues an in-progress divider drag given the pointer's movement
    /// since the last event.
    pub fn drag_by(&mut self, delta: (f32, f32)) {
        let Some((split, orientation, axis_extent)) = self.dragging else {
            return;
        };
        if axis_extent <= 0.0 {
            return;
        }
        let pixel_delta = match orientation {
            Orientation::Horizontal => delta.0,
            Orientation::Vertical => delta.1,
        };
        if crate::verbose::is_verbose(crate::verbose::Category::Mouse) {
            eprintln!(
                "mouse: drag_by delta={delta:?} pixel_delta={pixel_delta} ratio_delta={}",
                pixel_delta / axis_extent
            );
        }
        self.layout.resize(split, pixel_delta / axis_extent);
        self.resize_panes_to_geometry();
    }

    /// Ends the in-progress divider drag, if any.
    pub fn end_drag(&mut self) {
        self.dragging = None;
    }

    /// Converts a window-pixel position to a 0-indexed `(col, row)` within
    /// `pane`'s grid, clamping to the pane's own content rect (below its
    /// title bar) — a drag that wanders outside the pane it started in
    /// should still report the boundary cell, not stop reporting or panic
    /// on an out-of-range index. Returns `None` if `pos` is above the
    /// content rect entirely (i.e. within the title bar itself) — that's
    /// chrome, not grid, so a click landing there shouldn't start a
    /// selection or a mouse report (callers still get click-to-focus and
    /// the context menu from `pane_at`, which uses the full pane rect).
    fn cell_at(&self, pane: PaneId, pos: (f32, f32)) -> Option<(usize, usize)> {
        let geometry = self.layout.geometry(self.area(), DIVIDER_THICKNESS);
        let pane_rect = geometry.panes.iter().find(|p| p.pane == pane)?.rect;
        let rect = Self::content_rect(pane_rect, self.cell);
        if pos.1 < rect.y {
            return None;
        }
        let x = pos.0.clamp(rect.x, (rect.x + rect.width - 1.0).max(rect.x));
        let y = pos.1.clamp(rect.y, (rect.y + rect.height - 1.0).max(rect.y));
        let col = ((x - rect.x) / self.cell.0).floor().max(0.0) as usize;
        let row = ((y - rect.y) / self.cell.1).floor().max(0.0) as usize;
        Some((col, row))
    }

    /// Whether a mouse-reporting gesture (press-to-release) is in progress.
    pub fn is_mouse_reporting(&self) -> bool {
        self.mouse_gesture.is_some()
    }

    /// Attempts to start forwarding a mouse press to whichever pane is under
    /// `pos`, if that pane's program has turned on mouse reporting. Returns
    /// whether it engaged — callers should skip their own click handling
    /// (e.g. starting a text selection) for this press when it did, since
    /// the grid cell the click landed on belongs to the program now, not
    /// local chrome.
    pub fn mouse_press(&mut self, pos: (f32, f32), button: crate::mouse::Button, modifiers: crate::mouse::Modifiers) -> bool {
        let Some(pane) = self.pane_at(pos) else { return false };
        let Some(mode) = self.panes.get(&pane).map(|s| s.screen().mode()) else { return false };
        if !crate::mouse::wants_report(mode, crate::mouse::Kind::Press, false) {
            return false;
        }
        let Some((col, row)) = self.cell_at(pane, pos) else { return false };
        let bytes = crate::mouse::encode(mode, crate::mouse::Kind::Press, button, col, row, modifiers);
        if let Some(session) = self.panes.get_mut(&pane)
            && let Err(err) = session.write_input(&bytes)
        {
            eprintln!("pane: failed to write mouse report: {err:#}");
        }
        self.mouse_gesture = Some((pane, button));
        true
    }

    /// Forwards a release for the pane a matching `mouse_press` gesture is
    /// still open for, ending the gesture. A no-op (returns `false`) if no
    /// gesture is open or it was for a different button.
    pub fn mouse_release(&mut self, pos: (f32, f32), button: crate::mouse::Button, modifiers: crate::mouse::Modifiers) -> bool {
        let Some((pane, gesture_button)) = self.mouse_gesture.take() else { return false };
        if gesture_button != button {
            return false;
        }
        let Some(mode) = self.panes.get(&pane).map(|s| s.screen().mode()) else { return false };
        let (col, row) = self.cell_at(pane, pos).unwrap_or((0, 0));
        let bytes = crate::mouse::encode(mode, crate::mouse::Kind::Release, button, col, row, modifiers);
        if let Some(session) = self.panes.get_mut(&pane)
            && let Err(err) = session.write_input(&bytes)
        {
            eprintln!("pane: failed to write mouse report: {err:#}");
        }
        true
    }

    /// Forwards ongoing pointer motion for an open `mouse_press` gesture, if
    /// its pane's program wants motion events (button-event or any-event
    /// tracking). Returns whether a report was actually sent.
    pub fn mouse_motion(&mut self, pos: (f32, f32), modifiers: crate::mouse::Modifiers) -> bool {
        let Some((pane, button)) = self.mouse_gesture else { return false };
        let Some(mode) = self.panes.get(&pane).map(|s| s.screen().mode()) else { return false };
        if !crate::mouse::wants_report(mode, crate::mouse::Kind::Motion, true) {
            return false;
        }
        let Some((col, row)) = self.cell_at(pane, pos) else { return false };
        let bytes = crate::mouse::encode(mode, crate::mouse::Kind::Motion, button, col, row, modifiers);
        if let Some(session) = self.panes.get_mut(&pane)
            && let Err(err) = session.write_input(&bytes)
        {
            eprintln!("pane: failed to write mouse report: {err:#}");
        }
        true
    }

    /// Whether an in-grid text-selection drag is in progress.
    pub fn is_selecting(&self) -> bool {
        self.selecting.is_some()
    }

    /// Starts a text selection in whichever pane is under `pos`, if any —
    /// the local-selection counterpart to `mouse_press`'s forwarded click,
    /// used when the pane's program hasn't turned on mouse reporting.
    /// Clears any selection left over in every other pane first — only one
    /// pane's selection is ever highlighted/copyable at a time. Returns
    /// whether a selection started.
    pub fn start_selection(&mut self, pos: (f32, f32)) -> bool {
        let Some(pane) = self.pane_at(pos) else { return false };
        let Some((col, row)) = self.cell_at(pane, pos) else { return false };
        for (other_pane, session) in self.panes.iter_mut() {
            if *other_pane != pane {
                session.clear_selection();
            }
        }
        let Some(session) = self.panes.get_mut(&pane) else { return false };
        session.start_selection(row, col);
        self.selecting = Some(pane);
        true
    }

    /// Extends the in-progress selection to `pos`, if one is active.
    pub fn update_selection(&mut self, pos: (f32, f32)) {
        let Some(pane) = self.selecting else { return };
        let Some((col, row)) = self.cell_at(pane, pos) else { return };
        if let Some(session) = self.panes.get_mut(&pane) {
            session.update_selection(row, col);
        }
    }

    /// Ends the in-progress selection, if any. A selection that never moved
    /// from its starting cell (a plain click, not a drag) is discarded
    /// rather than left highlighting a single character; anything else is
    /// copied to the system clipboard, so a drag-select is immediately
    /// pasteable elsewhere — the only cross-platform-portable notion of
    /// "copyable" available here (Windows has no X11 PRIMARY selection to
    /// mirror into instead).
    pub fn end_selection(&mut self) {
        let Some(pane) = self.selecting.take() else { return };
        let Some(session) = self.panes.get_mut(&pane) else { return };
        if session.selection_is_empty() {
            session.clear_selection();
            return;
        }
        let Some(text) = session.screen().selection_to_string() else { return };
        Self::copy_to_clipboard(text);
    }

    /// Copies `pane`'s current selection to the system clipboard, if it has
    /// one — the terminal context menu's explicit "Copy" action. Shares the
    /// same clipboard write `end_selection` does automatically on a
    /// drag-release; a selection can still be sitting there, highlighted,
    /// well after the drag that created it ended, which is exactly when a
    /// right-click-to-copy is useful.
    pub fn copy_selection(&mut self, pane: PaneId) {
        let Some(session) = self.panes.get_mut(&pane) else { return };
        if session.selection_is_empty() {
            return;
        }
        let Some(text) = session.screen().selection_to_string() else { return };
        Self::copy_to_clipboard(text);
    }

    fn copy_to_clipboard(text: String) {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                if let Err(err) = clipboard.set_text(text) {
                    eprintln!("clipboard: failed to set text: {err:#}");
                }
            }
            Err(err) => eprintln!("clipboard: failed to open: {err:#}"),
        }
    }

    /// Reads the system clipboard and writes it straight to `pane`'s shell,
    /// as if typed — the terminal context menu's "Paste". A plain paste,
    /// not bracketed-paste-escaped: v1 scope matches most simple terminal
    /// emulators' default paste behavior, not iTerm2/kitty's opt-in
    /// bracketed paste mode that guards against pasted text executing as
    /// commands on its own newlines.
    pub fn paste_into_pane(&mut self, pane: PaneId) {
        let text = match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.get_text()) {
            Ok(text) => text,
            Err(err) => {
                eprintln!("clipboard: failed to read text: {err:#}");
                return;
            }
        };
        if let Some(session) = self.panes.get_mut(&pane)
            && let Err(err) = session.write_input(text.as_bytes())
        {
            eprintln!("failed to write pasted input to pane: {err:#}");
        }
    }

    /// Draws every visible pane's screen contents, dividers, and the
    /// focused pane's cursor, then presents the frame.
    ///
    /// Returns `false` if the layout has no panes left — a pane whose shell
    /// exits on its own (the user typed `exit`, not an app-level close) is
    /// closed automatically here, same as an explicit close action; the
    /// caller should quit when the last one goes.
    pub fn redraw(&mut self) -> bool {
        self.poll_config_reload();
        // Live-previews the settings panel's in-progress edits — applied
        // through the exact same path (`apply_settings`) a hot-reloaded
        // config file already goes through, so a font/color/transparency
        // change dragged in the panel shows up immediately instead of only
        // after Save, and reverts the instant the panel closes without
        // saving (see `ui_request.settings_cancelled` below) rather than
        // leaving a preview applied with nothing backing it. Read before
        // any of this frame's own rendering, since the grid is drawn
        // *before* `self.ui.show()` runs each frame — by the time that
        // call returns with this frame's edits, it's too late for this
        // frame's own grid to reflect them.
        if let Some(preview) = self.ui.live_preview(&self.settings) {
            self.apply_settings(preview);
        }
        if self.foreground_processes.maybe_refresh() && crate::verbose::is_verbose(crate::verbose::Category::Foreground) {
            // One line per pane, right after each scan (every ~500ms) —
            // enough to see the sequence of transitions without spamming
            // once per frame.
            for (pane, session) in &self.panes {
                let shell_pid = session.shell_pid();
                let foreground_pgid = session.foreground_pgid();
                let name = self.foreground_processes.name_for(shell_pid, foreground_pgid);
                eprintln!(
                    "foreground: {pane:?} shell_pid={shell_pid:?} foreground_pgid={foreground_pgid:?} name={name:?}"
                );
            }
        }

        let mut exited = Vec::new();
        for (pane, session) in self.panes.iter_mut() {
            session.pump();
            if session.has_exited() {
                exited.push(*pane);
            }
        }
        for pane in exited {
            if !self.close_pane(pane) {
                return false;
            }
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return true;
            }
            wgpu::CurrentSurfaceTexture::Timeout
            | wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Validation => return true,
        };

        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let geometry = self.layout.geometry(self.area(), DIVIDER_THICKNESS);
        let cell = self.cell;
        let focused = self.focused;
        // Needed early: the default-background fallback for cells left at
        // their default color (`color::resolve`) has to match the pane's
        // actual ambient background, not some other fixed value, or a
        // "colored" background rect would visibly seam against the real
        // one drawn behind it.
        let background_rgb = self.settings.appearance.background_rgb();
        // The cursor and selection highlight both use the user's chosen
        // accent color (Settings' "Accent color") rather than a fixed
        // constant — unlike the broadcast-target border, which is a fixed
        // semantic signal, these are interactive/focus highlights, the
        // category the accent color exists to theme.
        let accent_rgb = self.settings.appearance.accent_rgb();
        let cursor_color = [accent_rgb[0], accent_rgb[1], accent_rgb[2], 0.5];
        let selection_color = [accent_rgb[0], accent_rgb[1], accent_rgb[2], 0.45];

        let mut rects: Vec<render::SolidRect> = geometry
            .dividers
            .iter()
            .map(|d| render::SolidRect {
                x: d.rect.x,
                y: d.rect.y,
                width: d.rect.width,
                height: d.rect.height,
                color: DIVIDER_COLOR,
            })
            .collect();

        // Broadcast indicator: a border on every pane currently receiving
        // input, when that's more than just the focused pane on its own.
        if self.router.broadcast_mode != router::BroadcastMode::Off {
            let all_panes = self.layout.panes();
            let targets = self.router.broadcast_targets(focused, &all_panes);
            for pane_rect in geometry.panes.iter().filter(|p| targets.contains(&p.pane)) {
                push_border(&mut rects, pane_rect.rect, BROADCAST_BORDER_THICKNESS, BROADCAST_BORDER_COLOR);
            }
        }

        let panes = &self.panes;
        let router = &self.router;
        let foreground_processes = &self.foreground_processes;
        let glyphs: Vec<render::GlyphCell> = geometry
            .panes
            .iter()
            .filter_map(|pane_rect| panes.get(&pane_rect.pane).map(|session| (pane_rect, session)))
            .flat_map(|(pane_rect, session)| {
                let full = pane_rect.rect;
                let origin = Self::content_rect(full, cell);
                let screen = session.screen();
                let cells = screen.visible_cells();

                let mut pane_glyphs = Vec::new();

                // Title bar: dark grey/light grey by default; a grouped
                // pane instead gets a color keyed off its group's name (see
                // `GROUP_COLOR_PALETTE`) with a contrast-computed text
                // color, and the group name left-aligned alongside the
                // centered title.
                let group = router.group_of(pane_rect.pane);
                let (title_bar_bg, title_bar_text) = match &group {
                    Some(g) => {
                        let bg = group_color(&g.0);
                        (bg, contrasting_text_color(bg))
                    }
                    None => (TITLE_BAR_BG, TITLE_BAR_TEXT_LIGHT),
                };
                rects.push(render::SolidRect {
                    x: full.x,
                    y: full.y,
                    width: full.width,
                    height: Self::title_bar_height(cell),
                    color: title_bar_bg,
                });

                // The close button reserves its own cell on the right —
                // excluded from the title's available width/centering
                // (not just drawn on top of it), so a long title is never
                // visually clipped underneath the button instead of
                // truncated before it.
                let close_button = Self::close_button_rect(full, cell);
                let title_area_width = (close_button.x - TITLE_BAR_PADDING - full.x).max(0.0);
                let max_chars = (title_area_width / cell.0).floor().max(0.0) as usize;
                let title_y = full.y + TITLE_BAR_PADDING;
                let foreground_name = foreground_processes
                    .name_for(session.shell_pid(), session.foreground_pgid())
                    .unwrap_or_else(|| "shell".to_string());
                let title: String = foreground_name.chars().take(max_chars).collect();
                let title_width = title.chars().count() as f32 * cell.0;
                let title_x = full.x + ((title_area_width - title_width) / 2.0).max(0.0);
                pane_glyphs.extend(title.chars().enumerate().map(|(i, c)| render::GlyphCell {
                    x: title_x + i as f32 * cell.0,
                    y: title_y,
                    c,
                    color: title_bar_text,
                }));
                if let Some(g) = &group {
                    let name: String = g.0.chars().take(max_chars).collect();
                    pane_glyphs.extend(name.chars().enumerate().map(|(i, c)| render::GlyphCell {
                        x: full.x + TITLE_BAR_PADDING + i as f32 * cell.0,
                        y: title_y,
                        c,
                        color: title_bar_text,
                    }));
                }
                // Horizontally centered within the button's own (now
                // square, wider-than-a-glyph-cell) box — not just placed
                // at its left edge the way a regular monospace character
                // is, since the box is deliberately wider than one glyph
                // advance now (see `close_button_rect`).
                let close_glyph_x = close_button.x + (close_button.width - cell.0) / 2.0;
                pane_glyphs.push(render::GlyphCell {
                    x: close_glyph_x,
                    y: title_y,
                    c: CLOSE_BUTTON_GLYPH,
                    color: title_bar_text,
                });

                // The cursor's tracked position is always against the live
                // screen — while scrolled back into history, it doesn't
                // correspond to anything currently visible, so it's left
                // out rather than drawn somewhere misleading.
                if pane_rect.pane == focused && !screen.is_scrolled_back() {
                    let (row, col) = screen.cursor();
                    rects_push_cursor(&mut rects, origin, cell, row, col, cursor_color);
                }

                if let Some(range) = screen.selection_range() {
                    let cols = (origin.width / cell.0) as usize;
                    push_selection(&mut rects, origin, cell, range, cols, selection_color);
                }

                for (row, row_cells) in cells.into_iter().enumerate() {
                    for (col, rc) in row_cells.into_iter().enumerate() {
                        // SGR reverse-video (`Flags::INVERSE`) swaps which
                        // side of the cell each color paints — handled by
                        // just swapping which raw `Color` feeds the fg vs.
                        // bg resolution below, rather than as a special
                        // case at the end.
                        let (fg_src, bg_src) =
                            if rc.flags.contains(pane::Flags::INVERSE) { (rc.bg, rc.fg) } else { (rc.fg, rc.bg) };
                        let x = origin.x + col as f32 * cell.0;
                        let y = origin.y + row as f32 * cell.1;

                        if !color::is_default_background(bg_src) {
                            let [r, g, b] = color::resolve(bg_src, rc.flags, false, background_rgb);
                            rects.push(render::SolidRect { x, y, width: cell.0, height: cell.1, color: [r, g, b, 1.0] });
                        }

                        if rc.c != ' ' {
                            let [r, g, b] = color::resolve(fg_src, rc.flags, true, rgb3(TEXT_COLOR));
                            pane_glyphs.push(render::GlyphCell { x, y, c: rc.c, color: [r, g, b, 1.0] });
                        }
                    }
                }

                pane_glyphs
            })
            .collect();

        // Forced fully opaque on WSL regardless of the configured level —
        // the surface was configured `Opaque` there (see `new`), so the
        // compositor ignores this alpha channel anyway; without also
        // clamping it here, the premultiplied shader math would still dim
        // every color by the configured level for no visible transparency
        // benefit (the compositor never blends it with anything).
        let transparency = if platform::is_wsl() { 1.0 } else { self.settings.appearance.transparency.clamp(0.0, 1.0) };
        let [bg_r, bg_g, bg_b] = background_rgb;
        let background =
            wgpu::Color { r: bg_r as f64, g: bg_g as f64, b: bg_b as f64, a: transparency as f64 };
        self.grid.render(
            &self.device,
            &self.queue,
            &view,
            (self.config.width, self.config.height),
            scaled_font_size(self.settings.appearance.font_size, self.window.scale_factor()),
            &self.settings.appearance.font_family,
            background,
            rects.into_iter(),
            glyphs.into_iter(),
        );

        let group_names = self.router.group_names();
        let (ui_request, ui_output) = self.ui.show(
            &self.window,
            self.router.broadcast_mode,
            |pane| self.router.group_of(pane).map(|g| g.0),
            &group_names,
            &self.settings,
        );
        if let Some(mode) = ui_request.set_broadcast_mode {
            self.router.broadcast_mode = mode;
        }
        if let Some((pane, orientation)) = ui_request.split {
            self.split_pane(pane, orientation);
        }
        if let Some((pane, name)) = ui_request.assign_to_group {
            self.router.assign_to_group(pane, name);
        }
        if let Some(pane) = ui_request.remove_from_group {
            self.router.remove_from_group(pane);
        }
        if let Some((pane, shell)) = ui_request.restart_shell {
            self.restart_pane_shell(pane, shell.as_deref());
        }
        if let Some(arrangement) = ui_request.arrange {
            self.arrange_panes(arrangement);
        }
        if let Some(pane) = ui_request.copy_selection {
            self.copy_selection(pane);
        }
        if let Some(pane) = ui_request.paste_clipboard {
            self.paste_into_pane(pane);
        }
        if let Some(new_config) = ui_request.settings_saved {
            // Reapplies through `apply_settings` rather than assuming
            // this frame's already-applied live preview is identical to
            // `new_config` (normally true, but not guaranteed if the
            // panel could ever change without going through the preview
            // path) — cheap, and it's the one place `saved_settings`
            // itself gets updated, which future Cancels revert to.
            self.apply_settings(new_config.clone());
            self.saved_settings = new_config;
        }
        if ui_request.settings_cancelled {
            self.apply_settings(self.saved_settings.clone());
        }
        // Unlike the title-bar close button (handled directly in
        // `main.rs`, outside of `redraw` entirely, the same way the
        // `Ctrl+Shift+W` chord is), a menu-driven close is only known
        // once `self.ui.show` returns here — so "closed the last pane"
        // has to be threaded through this function's own return value
        // instead of exiting immediately.
        let mut quit_after_present = false;
        if let Some(pane) = ui_request.close_pane
            && !self.close_pane(pane)
        {
            quit_after_present = true;
        }

        let mut ui_encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        self.ui.render(
            &self.device,
            &self.queue,
            &mut ui_encoder,
            &view,
            (self.config.width, self.config.height),
            self.window.scale_factor() as f32,
            ui_output,
        );
        self.queue.submit(Some(ui_encoder.finish()));

        self.window.pre_present_notify();
        frame.present();
        !quit_after_present
    }
}

/// Starts watching `config_path`'s parent directory (not the file itself —
/// an editor that saves via temp-file-plus-rename can otherwise orphan a
/// watch on the file's original inode) for changes, reporting each one on
/// the returned channel. `None` if the directory couldn't be created or the
/// platform watcher couldn't start — hot reload is best-effort and never a
/// reason to fail startup.
/// Which wgpu backend(s) to allow. Windows is pinned to DX12 specifically —
/// see the comment where this is called — everywhere else keeps wgpu's own
/// default ("try every backend compiled in").
#[cfg(target_os = "windows")]
fn platform_backends() -> wgpu::Backends {
    wgpu::Backends::DX12
}

#[cfg(not(target_os = "windows"))]
fn platform_backends() -> wgpu::Backends {
    wgpu::Backends::default()
}

fn watch_config_dir(config_path: &std::path::Path) -> Option<(notify::RecommendedWatcher, Receiver<()>)> {
    let dir = config_path.parent()?;
    if let Err(err) = std::fs::create_dir_all(dir) {
        eprintln!("config: failed to create config directory {}: {err:#}", dir.display());
        return None;
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if result.is_ok() {
            let _ = tx.send(());
        }
    }) {
        Ok(watcher) => watcher,
        Err(err) => {
            eprintln!("config: failed to start a file watcher, hot reload disabled: {err:#}");
            return None;
        }
    };

    if let Err(err) = watcher.watch(dir, notify::RecursiveMode::NonRecursive) {
        eprintln!("config: failed to watch {}, hot reload disabled: {err:#}", dir.display());
        return None;
    }

    Some((watcher, rx))
}

/// Picks a title bar background for a group from `GROUP_COLOR_PALETTE`,
/// keyed by a hash of its name — the same name always lands on the same
/// color (stable across reloads/restarts and independent of creation
/// order), rather than a fresh random pick each time a group is created,
/// which would make a group's visual identity change on every rename or
/// reassignment round-trip.
fn group_color(name: &str) -> [f32; 4] {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    GROUP_COLOR_PALETTE[(hasher.finish() as usize) % GROUP_COLOR_PALETTE.len()]
}

/// Approximates a linear 0.0–1.0 color channel's *displayed* brightness.
/// The swapchain format is `Bgra8UnormSrgb` (confirmed via egui's own
/// startup log line) — an sRGB-aware target, which means the GPU always
/// gamma-encodes whatever a shader writes on its way to the screen. A
/// color that looks like a moderate 0.0–1.0 value in the source displays
/// noticeably *brighter* once that encoding happens (sRGB's curve boosts
/// mid-range values well above their linear input — linear 0.5 displays
/// close to 0.735). `contrasting_text_color` needs this, not the raw
/// value, to judge how bright a background will actually look.
fn srgb_encode(linear: f32) -> f32 {
    if linear <= 0.003_130_8 { linear * 12.92 } else { 1.055 * linear.powf(1.0 / 2.4) - 0.055 }
}

/// Light or dark title bar text, whichever contrasts with `bg` — perceived
/// luminance (the standard `0.299r + 0.587g + 0.114b` weighting, not a
/// straight average, since human vision is far more sensitive to green
/// than red or blue) computed on the *displayed* (sRGB-encoded) color, not
/// the raw linear input — using the raw value was consistently judging
/// colors as darker than they actually render, so brighter backgrounds
/// were keeping light text instead of flipping to dark.
fn contrasting_text_color(bg: [f32; 4]) -> [f32; 4] {
    let (r, g, b) = (srgb_encode(bg[0]), srgb_encode(bg[1]), srgb_encode(bg[2]));
    let luminance = 0.299 * r + 0.587 * g + 0.114 * b;
    if luminance > 0.5 { TITLE_BAR_TEXT_DARK } else { TITLE_BAR_TEXT_LIGHT }
}

fn rects_push_cursor(rects: &mut Vec<render::SolidRect>, origin: Rect, cell: (f32, f32), row: usize, col: usize, color: [f32; 4]) {
    rects.push(render::SolidRect {
        x: origin.x + col as f32 * cell.0,
        y: origin.y + row as f32 * cell.1,
        width: cell.0,
        height: cell.1,
        color,
    });
}

/// Emits highlight rects for `range` within a pane at `origin` — one per
/// row it spans, each covering the full row except the first/last row of a
/// multi-row (non-block) selection, which start/end at the selection's own
/// boundary column instead. Mirrors how `alacritty_terminal`'s own
/// `SelectionRange` is meant to be interpreted for rendering (see its
/// `contains`/`contains_cell` doc comments).
fn push_selection(
    rects: &mut Vec<render::SolidRect>,
    origin: Rect,
    cell: (f32, f32),
    range: pane::SelectionRange,
    cols: usize,
    color: [f32; 4],
) {
    let start_row = range.start.line.0.max(0) as usize;
    let end_row = range.end.line.0.max(0) as usize;
    let last_col = cols.saturating_sub(1);

    for row in start_row..=end_row {
        let (from_col, to_col) = if range.is_block || start_row == end_row {
            (range.start.column.0, range.end.column.0)
        } else if row == start_row {
            (range.start.column.0, last_col)
        } else if row == end_row {
            (0, range.end.column.0)
        } else {
            (0, last_col)
        };
        if to_col < from_col {
            continue;
        }

        rects.push(render::SolidRect {
            x: origin.x + from_col as f32 * cell.0,
            y: origin.y + row as f32 * cell.1,
            width: (to_col - from_col + 1) as f32 * cell.0,
            height: cell.1,
            color,
        });
    }
}

/// Emits a `thickness`-wide outline around `rect` as four solid rects (top,
/// bottom, left, right edges), inset so the border sits just inside the
/// pane rather than overlapping the divider.
fn push_border(rects: &mut Vec<render::SolidRect>, rect: Rect, thickness: f32, color: [f32; 4]) {
    rects.push(render::SolidRect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: thickness,
        color,
    });
    rects.push(render::SolidRect {
        x: rect.x,
        y: rect.y + rect.height - thickness,
        width: rect.width,
        height: thickness,
        color,
    });
    rects.push(render::SolidRect {
        x: rect.x,
        y: rect.y,
        width: thickness,
        height: rect.height,
        color,
    });
    rects.push(render::SolidRect {
        x: rect.x + rect.width - thickness,
        y: rect.y,
        width: thickness,
        height: rect.height,
        color,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_color_is_deterministic_for_the_same_name() {
        assert_eq!(group_color("backend"), group_color("backend"));
    }

    #[test]
    fn srgb_encode_matches_known_reference_points() {
        // Standard IEC 61966-2-1 reference points.
        assert!((srgb_encode(0.0) - 0.0).abs() < 1e-6);
        assert!((srgb_encode(1.0) - 1.0).abs() < 1e-6);
        assert!((srgb_encode(0.5) - 0.735).abs() < 0.005);
    }

    #[test]
    fn contrast_accounts_for_srgb_display_brightness_not_just_raw_linear_value() {
        // A color the raw (pre-gamma) formula would call "moderate" but
        // that displays bright once sRGB-encoded — exactly the palette
        // entries the developer reported as not flipping to dark text.
        // Raw luminance here is ~0.474 (< 0.5, would wrongly pick light
        // text); the sRGB-aware calculation must pick dark instead.
        let teal = [0.20, 0.60, 0.55, 1.0];
        assert_eq!(contrasting_text_color(teal), TITLE_BAR_TEXT_DARK);
    }

    #[test]
    fn contrast_still_picks_light_text_for_a_genuinely_dark_color() {
        assert_eq!(contrasting_text_color(TITLE_BAR_BG), TITLE_BAR_TEXT_LIGHT);
    }
}
