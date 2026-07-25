# Vendored dependencies

## `wgpu-hal-29.0.4`

A local copy of `wgpu-hal` 29.0.4 (MIT OR Apache-2.0), patched with one
targeted fix, wired in via `[patch.crates-io]` in the workspace
`Cargo.toml`. Not submitted upstream — deliberately not yet, until the fix
is fully understood and verified on real hardware, not just reasoned about
from source. This is a fork for our own use, not a contribution in
progress.

### The problem

Enabling real per-pixel window transparency on Windows requires wgpu's
DirectComposition-backed swapchain path (`Dx12BackendOptions.
presentation_system = Dx12SwapchainKind::DxgiFromVisual`) — a plain
window-handle swapchain (what you get by default) only ever reports
`CompositeAlphaMode::Opaque` on Windows, on any backend, so there is no way
to get real transparency without it.

Turning that option on crashed on first run:

```
thread 'main' panicked ...
wgpu error: Validation Error
Caused by:
  In Surface::configure
    Invalid surface
```

With logging enabled (see `env_logger` init in `crates/app/src/main.rs`,
added specifically to chase this down), the real underlying error was:

```
ERROR wgpu_hal::dx12: SwapChain creation error: The application made a call
that is invalid. ... (0x887A0001)
```

`0x887A0001` is `DXGI_ERROR_INVALID_CALL`, raised from
`IDXGIFactory2::CreateSwapChainForComposition`.

### The cause

`wgpu-hal`'s DX12 `Surface::configure` (`src/dx12/mod.rs`) always sets
`DXGI_SWAP_CHAIN_FLAG_ALLOW_TEARING` on the swapchain description whenever
the adapter/factory reports tearing support at all, regardless of which
kind of swapchain is being created. Per Microsoft's own DXGI
documentation, that flag is valid only for swapchains created via
`CreateSwapChainForHwnd` — every DirectComposition-based creation path
(`CreateSwapChainForComposition`, used for `Visual`/`VisualFromWndHandle`/
`SwapChainPanel`; `CreateSwapChainForCompositionSurfaceHandle`, used for
`SurfaceHandle`) rejects it outright. Any adapter that supports tearing —
which is effectively every modern GPU — hits this. The same code is
present unchanged in wgpu-hal 30.0.0 too, so this isn't fixed by a version
bump.

In short: `Dx12SwapchainKind::DxgiFromVisual` (the whole point of turning
this on) is broken out of the box on typical modern hardware, not just
ours.

### The fix

In `src/dx12/mod.rs`'s `configure`, only set the tearing flag when
`self.target` is `SurfaceTarget::WndHandle` — the one creation path that
actually supports it. Search this vendored copy for "local patch" to find
the exact change.

This fix was necessary but not sufficient on its own — two more issues
were hiding behind it, below.

### A second issue behind the first (not a wgpu-hal bug — ours)

With the tearing flag fixed, swapchain creation still failed with the same
`DXGI_ERROR_INVALID_CALL`. Installing the Windows "Graphics Tools" optional
feature (provides the D3D12 debug layer) and capturing its output with
Sysinternals DebugView gave the real, specific message this time:

```
DXGI ERROR: IDXGIFactory::CreateSwapChainForComposition: Composition
SwapChains do not support the DXGI_ALPHA_MODE_STRAIGHT AlphaMode.
```

This one wasn't a `wgpu-hal` bug — it was us picking the wrong
`wgpu::CompositeAlphaMode`. `crates/app/src/graphics.rs` had been
requesting `PostMultiplied` (DXGI `STRAIGHT`) because that's what
`crates/render`'s pipeline produced at the time. Composition swapchains
only accept `PREMULTIPLIED`. Fixed at the app/render level, not here:
`crates/render`'s pipeline now uses `PREMULTIPLIED_ALPHA_BLENDING` and its
shader outputs premultiplied color, and `graphics.rs` now requests
`CompositeAlphaMode::PreMultiplied`. See those crates' own history for
detail — nothing further to patch in this vendored copy for it.

### A third issue: resize doesn't recomposite (this one is back in `wgpu-hal`)

With both of the above fixed, the window rendered correctly at launch —
but resizing it larger left the original rect showing frozen content while
the newly exposed area showed raw desktop passthrough (confirmed visually:
a screenshot of a resized window showed exactly this split).

Root cause, found by re-reading `Surface::configure`'s resize branch (the
`Some(sc) => { ... }` arm, taken whenever a swapchain already exists): it
only calls `ResizeBuffers` on the DXGI swapchain. That branch is *already*
flagged incomplete by wgpu-hal's own author — the line right above it
reads `//Note: this path doesn't properly re-initialize all of the
things`. For `VisualFromWndHandle`, what it's missing is a fresh
`IDCompositionVisual::SetContent` + `IDCompositionDevice::Commit` —
`Commit` is what actually pushes a pending change (here: "this visual's
content is now a different size") out to the desktop compositor.
`ResizeBuffers` alone resizes the DXGI-level buffers just fine, but the
compositor is never told, so it keeps displaying whatever was last
committed — matching the symptom exactly: a static, frozen region at the
old size, with genuinely nothing composited outside it.

### Fix

In the `Some(sc)` branch of `configure`, after a successful
`ResizeBuffers`, re-fetch the existing `DCompState` (via `get_or_init` —
already initialized at this point, so this just re-acquires the same
`IDCompositionDevice`/`Visual`, no new device gets created) and call
`SetContent`/`Commit` again, the same as the initial-creation path already
does. Search this vendored copy for "local patch" to find the exact
change (there are now two, both in `configure`).

This is a real, worthwhile fix on its own — it matches what the initial-
creation path already does, and upstream's own comment already flags the
resize branch as incomplete — but it turned out **not** to be the cause of
the resize symptom described below. Left in place regardless.

### A fourth issue: it wasn't wgpu-hal at all — it was `winit`

With temporary `warn`-level logging added around the fix above, testing
confirmed `SetContent`/`Commit` both run and succeed on every resize, with
correct dimensions — and the frozen-rect symptom was still there. That
ruled the `Commit` theory out as the actual cause, even though it was a
real gap.

The real cause: `winit`'s Windows backend (`platform_impl/windows/
window.rs`, `on_create`) calls `DwmEnableBlurBehindWindow` — an older,
GDI-redirection-surface-based transparency mechanism — automatically for
any window created with `transparent: true`, *unless* the window was also
created with `WS_EX_NOREDIRECTIONBITMAP` set. This app's window has always
been created transparent without that flag, so both mechanisms were
active on the same window at once: our DirectComposition visual (correct,
resize-aware after the fix above) and DWM's own legacy blur-behind surface
(created once, at window-creation size, with no resize awareness at all).
The frozen rectangle was the blur-behind surface, not anything on the
DirectComposition side — which is exactly why fixing DirectComposition's
resize handling made no visible difference.

### Fix (this one isn't in the vendored copy — it's `crates/app/src/main.rs`)

Set `WS_EX_NOREDIRECTIONBITMAP` on window creation on Windows, via winit's
own public `WindowAttributesExtWindows::with_no_redirection_bitmap(true)`
— this is also Microsoft's documented recommendation for any app that
presents through its own swapchain rather than GDI, so it's not really a
workaround, it's using the intended API for this situation. See
`platform_window_attributes` in `main.rs`.

### Status

- Four issues found and fixed on the road to Windows transparency: tearing
  flag, alpha mode, a real-but-not-the-actual-cause resize/Commit gap, and
  the actual cause (two conflicting transparency mechanisms active on the
  same window). Two live in this vendored `wgpu-hal` copy, one in
  `crates/render`, one in `crates/app` itself.
- Every step reasoned from real evidence — a runtime HRESULT, a D3D12
  debug-layer message, a screenshot, and diagnostic logging that ruled a
  plausible-looking theory out — not guessed from documentation recall.
  Two of the four theories along the way were wrong or incomplete despite
  seeming solid at the time; each was confirmed wrong by testing rather
  than assumed fixed.
- Still not submitted upstream, same reasoning as before — and note the
  `winit` fix doesn't need upstreaming at all, since it's just using an
  API winit already exposes for exactly this purpose.
