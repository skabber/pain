//! Application entrypoint: a winit window rendered via wgpu.

mod color;
mod foreground_process;
mod graphics;
mod mouse;
mod pane_session;
mod platform;
mod session_cwd;
mod ui;
mod verbose;

use std::sync::Arc;

use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
#[cfg(target_os = "linux")]
use winit::platform::x11::EventLoopBuilderExtX11;
use winit::window::{CursorIcon, Window, WindowId};

use layout::Orientation;

use graphics::Graphics;

fn main() -> anyhow::Result<()> {
    // `wgpu`/`wgpu-hal` report real backend failures (a DirectComposition
    // call failing, a surface misconfiguration, ...) through the `log`
    // crate, not by returning a message we can catch ourselves — without a
    // logger installed those `log::error!`/`log::warn!` calls go nowhere,
    // silently, and a wgpu-side failure surfaces as a bare "Invalid
    // surface"/"Validation Error" panic with none of the actual detail.
    // Defaults to `warn` (errors and warnings only) when `RUST_LOG` isn't
    // set, rather than needing that environment variable remembered on top
    // of this app's own `--verbose` flag just to see a real backend error.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let args: Vec<String> = std::env::args().collect();
    if let Some(flag) = args.iter().find(|a| *a == "--verbose" || *a == "-v" || a.starts_with("--verbose=")) {
        verbose::set_verbose(flag.strip_prefix("--verbose="));
    }

    let event_loop = build_event_loop()?;
    // Poll rather than wait: PTY output can arrive at any time, not just in
    // response to a window event, so the frame needs to keep coming around
    // to pick it up. A dedicated wake channel would avoid the busy loop, but
    // that's an efficiency concern for later, not this de-risking milestone.
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Builds the event loop, forcing X11 under WSL.
///
/// WSLg's Wayland compositor drops the client connection on focus changes
/// (surfaced by winit as a fatal `EventLoopError`, killing the whole app —
/// observed in development). XWayland, forced via winit's X11 backend, is
/// far more stable there. Native Linux desktops are unaffected and keep
/// winit's normal Wayland-preferred autodetection.
#[cfg(target_os = "linux")]
fn build_event_loop() -> Result<EventLoop<()>, winit::error::EventLoopError> {
    let mut builder = EventLoop::builder();
    if platform::is_wsl() {
        builder.with_x11();
    }
    builder.build()
}

#[cfg(not(target_os = "linux"))]
fn build_event_loop() -> Result<EventLoop<()>, winit::error::EventLoopError> {
    EventLoop::new()
}

/// `WS_EX_NOREDIRECTIONBITMAP` — skips the classic GDI-based redirection
/// surface Windows normally backs every window with. This app renders
/// through its own DirectComposition-backed swapchain (see `Graphics::
/// new`'s wgpu backend setup, needed for real window transparency there);
/// leaving the redirection bitmap in place gives the window *two*
/// independent backing surfaces — winit's own `DwmEnableBlurBehindWindow`
/// call (made automatically for a transparent window, unless this flag is
/// set) targets that legacy surface, not our DirectComposition visual, and
/// the two don't stay in sync on resize. Diagnosed from a real symptom:
/// after fixing DirectComposition's own resize handling, resizing the
/// window still left a frozen, opaque rectangle at the old size — with
/// confirmed-successful `SetContent`/`Commit` calls on our side, meaning
/// whatever was still showing frozen content wasn't coming from our visual
/// at all. This is also Microsoft's own documented recommendation for any
/// app presenting through its own swapchain instead of GDI.
#[cfg(target_os = "windows")]
fn platform_window_attributes(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use winit::platform::windows::WindowAttributesExtWindows;
    attributes.with_no_redirection_bitmap(true)
}

#[cfg(not(target_os = "windows"))]
fn platform_window_attributes(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attributes
}

/// The window icon shown in the taskbar, alt-tab switcher, and (on some
/// desktops) the title bar — decoded from the same `assets/pain-64.png`
/// the installed icon theme uses, so there's one source of truth rather
/// than a separately-maintained copy. 64px is the useful middle: large
/// enough that a compositor scaling it down still looks clean, small
/// enough to keep the decode trivial.
///
/// Returns `None` rather than failing the launch if the icon can't be
/// decoded — a missing icon is a cosmetic problem, not a reason to refuse
/// to open a terminal.
fn window_icon() -> Option<winit::window::Icon> {
    let bytes = include_bytes!("../../../assets/pain-64.png");
    // `Cursor`, not the slice directly: `png::Decoder` needs `BufRead +
    // Seek`, and `&[u8]` is only the former.
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes.as_slice()));
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    // `Icon::from_rgba` requires exactly 8-bit RGBA; the asset is
    // generated that way, but a future re-export could quietly change it,
    // and silently drawing garbage pixels would be worse than no icon.
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    winit::window::Icon::from_rgba(buf, info.width, info.height).ok()
}

/// Matches the `StartupWMClass` in `assets/pain.desktop`, which is how a
/// Linux desktop associates the running window with its installed
/// `.desktop` entry (and therefore its icon). winit would otherwise
/// derive this from `argv[0]`'s basename, which happens to be right today
/// but silently breaks the association if the binary is ever launched
/// through a symlink or renamed wrapper.
#[cfg(all(unix, not(target_os = "macos")))]
fn with_app_id(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    use winit::platform::wayland::WindowAttributesExtWayland;
    use winit::platform::x11::WindowAttributesExtX11;
    // Both traits define `with_name`, so each call is fully qualified —
    // the window needs the id set for whichever backend it ends up on,
    // and setting the other is harmless.
    let attributes = WindowAttributesExtX11::with_name(attributes, "pain", "pain");
    WindowAttributesExtWayland::with_name(attributes, "pain", "pain")
}

#[cfg(not(all(unix, not(target_os = "macos"))))]
fn with_app_id(attributes: winit::window::WindowAttributes) -> winit::window::WindowAttributes {
    attributes
}

#[derive(Default)]
struct App {
    graphics: Option<Graphics>,
    modifiers: ModifiersState,
    cursor_pos: (f32, f32),
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.graphics.is_some() {
            return;
        }

        // Loaded once, up front: the window's saved size (if any) has to be
        // requested at creation time, before `Graphics::new` — which is
        // also where the rest of a restored session (layout, panes, cwds)
        // actually gets used — ever runs.
        let session = session::Session::load(&session::Session::default_path());
        if crate::verbose::is_verbose(verbose::Category::General) {
            eprintln!("session: loaded {session:?}");
        }

        // Requested transparent-capable regardless of the current config
        // (except on WSL — see below): this attribute can't be changed
        // after creation, but the transparency *level* (`Graphics`'s
        // clear-color alpha) needs to stay hot-reloadable at runtime
        // (Milestone 6.2), so the window itself has to support it
        // unconditionally up front.
        let mut attributes = Window::default_attributes().with_title("pain").with_window_icon(window_icon());
        attributes = with_app_id(attributes);
        if let Some(s) = &session {
            attributes = attributes.with_inner_size(winit::dpi::PhysicalSize::new(s.window.width, s.window.height));
        }
        if !platform::is_wsl() {
            // On X11 this alone makes winit request a 32-bit ARGB visual
            // for the window — a window-creation-time property, entirely
            // separate from whatever `CompositeAlphaMode` the swapchain
            // later requests (`Graphics::new` already skips requesting a
            // transparent-capable one on WSL). Found the hard way: WSLg
            // kept compositing the window with alpha by default purely
            // because of this ARGB visual, even after the swapchain side
            // was fixed to ask for `Opaque` — the two are independent
            // mechanisms, same as the Windows DirectComposition-vs-
            // redirection-bitmap issue earlier this session, and both
            // halves have to agree for transparency to actually turn off.
            attributes = attributes.with_transparent(true);
        }
        let attributes = platform_window_attributes(attributes);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };

        match Graphics::new(window, session) {
            Ok(graphics) => {
                graphics.window().request_redraw();
                self.graphics = Some(graphics);
            }
            Err(err) => {
                eprintln!("failed to initialize GPU context: {err:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(graphics) = &mut self.graphics else {
            return;
        };

        // Every event goes to the UI overlay first, so it stays in sync
        // (focus, hover, etc.) even for events it doesn't end up consuming.
        // Only pointer/keyboard input actually needs the consumed check —
        // a click or keypress landing on the overlay shouldn't also reach
        // the pane grid or divider hit-testing underneath it.
        let mut ui_consumed = graphics.ui_consume_event(&event);
        // `egui-winit` marks *every* Tab keypress "consumed" unconditionally
        // — it's hardcoded as egui's own focus-cycling convention ("Tab
        // always consumes", regardless of whether anything is even
        // focusable) — which silently ate Tab completion in every shell:
        // `key_bytes` below already maps Tab to `\t` correctly, but this
        // flag being permanently true meant it was never reached. Only
        // override it while our own overlay has nothing open to cycle
        // focus between; a context menu/settings panel text field still
        // gets normal Tab behavior.
        if ui_consumed && is_tab_key(&event) && !graphics.ui_wants_keyboard_focus() {
            ui_consumed = false;
        }
        if ui_consumed {
            graphics.window().request_redraw();
        }
        if verbose::is_verbose(verbose::Category::Mouse) && matches!(event, WindowEvent::MouseInput { .. }) {
            eprintln!("mouse: {event:?} ui_consumed={ui_consumed}");
        }

        match event {
            WindowEvent::CloseRequested => {
                graphics.save_session();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                graphics.resize(size.width, size.height);
                graphics.window().request_redraw();
            }
            // The window was dragged to a monitor with a different DPI
            // scaling setting, or the OS-level scale changed — font size
            // is scaled by this factor at measurement/render time (see
            // `graphics::scaled_font_size`), so it has to be recomputed
            // here rather than staying stuck at whatever the previous
            // monitor's scale factor produced.
            WindowEvent::ScaleFactorChanged { .. } => {
                graphics.rescale();
                graphics.window().request_redraw();
            }
            WindowEvent::RedrawRequested => {
                if !graphics.redraw() {
                    graphics.save_session();
                    event_loop.exit();
                }
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::KeyboardInput { event, .. } if !ui_consumed => {
                let chord_result =
                    winit_chord(&event, self.modifiers).and_then(|chord| graphics.dispatch_chord(chord));
                match chord_result {
                    Some(true) => graphics.window().request_redraw(),
                    Some(false) => {
                        graphics.save_session();
                        event_loop.exit();
                    }
                    None => {
                        if let Some(bytes) = key_bytes(&event, self.modifiers)
                            && let Err(err) = graphics.send_input(&bytes)
                        {
                            eprintln!("failed to write input to pane: {err:#}");
                        }
                    }
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pos = (position.x as f32, position.y as f32);
                if verbose::is_verbose(verbose::Category::Mouse) {
                    eprintln!("mouse: cursor moved to {pos:?}, dragging={}", graphics.is_dragging());
                }
                // Divider drag/hover only applies when the overlay isn't
                // handling the event, but `cursor_pos` itself always needs
                // to track real movement — otherwise a drag started right
                // after the pointer leaves the overlay would compute its
                // first delta against a stale position.
                if !ui_consumed {
                    if graphics.is_dragging() {
                        let delta = (pos.0 - self.cursor_pos.0, pos.1 - self.cursor_pos.1);
                        graphics.drag_by(delta);
                        graphics.window().request_redraw();
                    } else if graphics.is_mouse_reporting() {
                        if graphics.mouse_motion(pos, mouse_modifiers(self.modifiers)) {
                            graphics.window().request_redraw();
                        }
                    } else if graphics.is_selecting() {
                        graphics.update_selection(pos);
                        graphics.window().request_redraw();
                    } else {
                        let icon = match graphics.divider_orientation_at(pos) {
                            Some(Orientation::Horizontal) => CursorIcon::EwResize,
                            Some(Orientation::Vertical) => CursorIcon::NsResize,
                            None => CursorIcon::Default,
                        };
                        graphics.window().set_cursor(icon);
                    }
                }
                self.cursor_pos = pos;
            }
            WindowEvent::MouseInput { state, button: MouseButton::Left, .. } if !ui_consumed => {
                if verbose::is_verbose(verbose::Category::Mouse) {
                    eprintln!("mouse: button {state:?} at {:?}", self.cursor_pos);
                }
                // A left click that isn't on the context menu itself (it
                // would have been `ui_consumed` if so) always just
                // dismisses an open menu, rather than also acting as a
                // normal pane/divider click — the same convention as most
                // context menus, so "clicking away" reads as one action.
                if graphics.close_context_menu() {
                    graphics.window().request_redraw();
                } else {
                    let modifiers = mouse_modifiers(self.modifiers);
                    match state {
                        ElementState::Pressed => {
                            // The title-bar close button always wins over
                            // every other press interpretation below —
                            // checked first, before even a divider grab,
                            // since it's drawn on top of everything else in
                            // the title bar and a click there should never
                            // also start a drag or change focus.
                            if let Some(pane) = graphics.close_button_at(self.cursor_pos) {
                                if !graphics.close_pane(pane) {
                                    graphics.save_session();
                                    event_loop.exit();
                                } else {
                                    graphics.window().request_redraw();
                                }
                                return;
                            }
                            // A press either grabs a divider, or focuses
                            // whichever pane it landed in and then either
                            // forwards the click as an escape sequence (if
                            // that pane's program turned on mouse
                            // reporting) or starts a local text selection
                            // otherwise. Never more than one of these: a
                            // divider isn't part of either pane it
                            // separates, and a click is either reported or
                            // selected, not both. Holding Shift always forces
                            // local selection, bypassing reporting entirely —
                            // the standard xterm escape hatch for selecting
                            // text in full-screen programs (vim, htop, ...)
                            // that would otherwise treat the click as input.
                            if !graphics.begin_drag(self.cursor_pos) {
                                let focus_changed = graphics.focus_at(self.cursor_pos);
                                let reported = !modifiers.shift
                                    && graphics.mouse_press(self.cursor_pos, mouse::Button::Left, modifiers);
                                let selecting = !reported && graphics.start_selection(self.cursor_pos);
                                if focus_changed || reported || selecting {
                                    graphics.window().request_redraw();
                                }
                            }
                        }
                        ElementState::Released => {
                            graphics.mouse_release(self.cursor_pos, mouse::Button::Left, modifiers);
                            graphics.end_drag();
                            graphics.end_selection();
                            graphics.window().request_redraw();
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } if !ui_consumed => {
                if verbose::is_verbose(verbose::Category::Mouse) {
                    eprintln!("mouse: wheel {delta:?} at {:?}", self.cursor_pos);
                }
                if graphics.scroll_at(self.cursor_pos, delta) {
                    graphics.window().request_redraw();
                }
            }
            WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Right, .. }
                if !ui_consumed =>
            {
                if verbose::is_verbose(verbose::Category::Mouse) {
                    eprintln!(
                        "mouse: right-click at {:?}, pane={:?}",
                        self.cursor_pos,
                        graphics.pane_at(self.cursor_pos)
                    );
                }
                // A right-click on a pane's title bar opens the
                // pane-management menu (Broadcast/Split/Arrange/Group/Swap
                // shell/Settings); anywhere else in the pane — the
                // terminal content itself — opens the copy/paste menu
                // instead.
                if !graphics.open_context_menu_at(self.cursor_pos) {
                    graphics.open_terminal_context_menu_at(self.cursor_pos);
                }
                graphics.window().request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(graphics) = &self.graphics {
            graphics.window().request_redraw();
        }
    }
}

/// Translates winit's current modifier state into `mouse::Modifiers`, for
/// encoding into a forwarded mouse report.
fn mouse_modifiers(modifiers: ModifiersState) -> mouse::Modifiers {
    mouse::Modifiers {
        shift: modifiers.shift_key(),
        alt: modifiers.alt_key(),
        ctrl: modifiers.control_key(),
    }
}

/// Whether `event` is a Tab keypress — see the Tab-key override in
/// `App::window_event`.
fn is_tab_key(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::KeyboardInput { event, .. } if event.logical_key == Key::Named(NamedKey::Tab)
    )
}

/// Translates a winit key press into a `router::Chord` candidate. Only
/// `Pressed` events with a single character or an arrow key can be chords
/// in v1's keymap (see `router::Keymap::terminator_defaults`) — everything
/// else (Enter, Tab, Escape, Backspace, ...) is never bound, so there's no
/// need to represent it as a `Chord` at all.
///
/// Whether the resulting chord is actually *bound* to anything is for
/// `Router::resolve` to decide, not this function — an unbound chord and a
/// non-chord key both end up falling through to `key_bytes` passthrough,
/// but for different reasons, and only one of them is this function's job.
fn winit_chord(event: &winit::event::KeyEvent, modifiers: ModifiersState) -> Option<router::Chord> {
    if event.state != ElementState::Pressed {
        return None;
    }

    let key = match &event.logical_key {
        Key::Character(s) => {
            let mut chars = s.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            router::Key::Char(c.to_ascii_lowercase())
        }
        Key::Named(NamedKey::ArrowUp) => router::Key::Up,
        Key::Named(NamedKey::ArrowDown) => router::Key::Down,
        Key::Named(NamedKey::ArrowLeft) => router::Key::Left,
        Key::Named(NamedKey::ArrowRight) => router::Key::Right,
        _ => return None,
    };

    Some(router::Chord {
        key,
        ctrl: modifiers.control_key(),
        shift: modifiers.shift_key(),
        alt: modifiers.alt_key(),
        logo: modifiers.super_key(),
    })
}

/// Translates a key press into the bytes to send to the pane's shell.
///
/// Bound chords are consumed by `Router::dispatch_chord` before this is
/// ever called — every key that reaches here is either unbound or not a
/// chord candidate at all, and passes straight through as raw input.
///
/// Named keys are matched before falling back to `event.text`, not after:
/// winit populates `event.text` per-platform from the OS's own text
/// composition (e.g. Windows' `WM_CHAR`), which for keys like Backspace can
/// disagree with the conventional terminal byte. Concretely, Windows
/// composes Backspace as BS (`0x08`), but sending that to cmd.exe's line
/// editor erases a whole word, not one character — terminals conventionally
/// send DEL (`0x7f`) for Backspace instead, precisely to avoid this. Letting
/// `event.text` win for named keys would silently prefer the OS's
/// composition over our deliberate convention.
///
/// Ctrl+letter is encoded to its control byte (Ctrl+A=1 .. Ctrl+Z=26) before
/// the `event.text` fallback too: holding Ctrl generally suppresses normal
/// text composition (so `event.text` would be empty anyway), and shells
/// depend on these bytes for basics like interrupting a running program
/// (Ctrl+C) or erasing a word (Ctrl+W).
fn key_bytes(event: &winit::event::KeyEvent, modifiers: ModifiersState) -> Option<Vec<u8>> {
    if event.state != winit::event::ElementState::Pressed {
        return None;
    }

    match &event.logical_key {
        Key::Named(NamedKey::Enter) => return Some(b"\r".to_vec()),
        Key::Named(NamedKey::Backspace) => return Some(vec![0x7f]),
        Key::Named(NamedKey::Tab) => return Some(b"\t".to_vec()),
        Key::Named(NamedKey::Escape) => return Some(vec![0x1b]),
        Key::Named(NamedKey::ArrowUp) => return Some(b"\x1b[A".to_vec()),
        Key::Named(NamedKey::ArrowDown) => return Some(b"\x1b[B".to_vec()),
        Key::Named(NamedKey::ArrowRight) => return Some(b"\x1b[C".to_vec()),
        Key::Named(NamedKey::ArrowLeft) => return Some(b"\x1b[D".to_vec()),
        _ => {}
    }

    if modifiers.control_key()
        && let Key::Character(s) = &event.logical_key
        && let Some(c) = s.chars().next().filter(|c| c.is_ascii_alphabetic())
        && s.chars().count() == 1
    {
        return Some(vec![c.to_ascii_uppercase() as u8 - b'A' + 1]);
    }

    if let Some(text) = &event.text
        && !text.is_empty()
    {
        return Some(text.as_bytes().to_vec());
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards a silent-failure mode: `window_icon` returns `None` on any
    /// decode problem (deliberately — a missing icon shouldn't stop the
    /// app launching), so a re-exported asset in the wrong format would
    /// ship with no window icon at all and nothing would say so. This
    /// caught exactly that once already: ImageMagick emits 16-bit PNGs by
    /// default, which the 8-bit RGBA requirement rejects.
    #[test]
    fn embedded_window_icon_actually_decodes() {
        assert!(window_icon().is_some(), "the embedded icon asset must decode to 8-bit RGBA");
    }
}
