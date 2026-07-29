# Nix package for pain. Standard callPackage pattern, so it works two ways:
#
#   1. Via this repo's flake output (recommended for dotfiles):
#        inputs.pain.url = "github:<owner>/pain";   # or "git+file:///home/jay/Projects/pain"
#        environment.systemPackages = [ inputs.pain.packages.x86_64-linux.default ];
#
#   2. Directly from a dotfiles flake with an explicit src:
#        pain = pkgs.callPackage /path/to/package.nix { src = pkgs.fetchFromGitHub { ... }; };
#
# NOTE: `src` has no default — callPackage would otherwise auto-fill it from
# a throwing nixpkgs alias (the old `src` package), so callers must pass it.
{
  lib,
  rustPlatform,
  pkg-config,
  makeWrapper,
  wayland,
  wayland-scanner,
  libxkbcommon,
  # Runtime-only below: winit/wgpu/arboard dlopen() these instead of linking
  # them (same reason the dev shell sets LD_LIBRARY_PATH), so they never show
  # up in the binary's DT_NEEDED and must be forced onto the wrapper's path.
  vulkan-loader,
  libglvnd,
  libX11,
  libXcursor,
  libXi,
  libxcb,
  src,
}:

let
  runtimeLibs = [
    vulkan-loader # libvulkan.so.1
    libglvnd # libEGL.so.1 / libGLX
    wayland # libwayland-client.so / libwayland-egl.so
    libxkbcommon # libxkbcommon.so / libxkbcommon-x11.so
    libX11 # libX11.so + libX11-xcb.so
    libXcursor
    libXi
    libxcb
  ];
in
rustPlatform.buildRustPackage {
  pname = "pain";
  version = "1.6.0"; # keep in sync with [workspace.package] in Cargo.toml

  # cleanSourceWith keeps local `nix build` / callPackage fast by not
  # copying build artifacts into the store; when src is the flake source
  # it's already git-clean and this is a no-op.
  src = lib.cleanSourceWith {
    src = src;
    filter =
      path: type:
      let
        base = baseNameOf path;
      in
      !(builtins.elem base [
        "target"
        ".direnv"
        ".git"
        "result"
      ]);
  };

  # The [patch.crates-io] wgpu-hal is a path dependency inside vendor/, so
  # `cargo vendor` (which is what this hash covers) skips it and leaves the
  # patch pointing at the in-tree path — no outputHashes entry needed, as
  # long as the src filter above keeps vendor/.
  cargoHash = "sha256-HXxPxe24iZOCRh6NY67d7SDQ6VFaSVS6C1nh6geOjx8=";

  # Only the app crate produces a binary; the other six workspace crates are
  # libraries it depends on.
  cargoBuildFlags = [
    "-p"
    "pain"
  ];

  nativeBuildInputs = [
    pkg-config # wayland-sys finds libwayland via pkg-config at build time
    wayland-scanner # wayland-sys runs it at build time to generate protocol code
    makeWrapper
  ];

  buildInputs = [
    wayland
    libxkbcommon
  ];

  # The dlopen()'d GUI libraries above can't be discovered by Nix's automatic
  # rpath fixup, so wrap the binary with LD_LIBRARY_PATH — same mechanism the
  # dev shell uses.
  postFixup = ''
    wrapProgram $out/bin/pain \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
  '';

  # Desktop integration, mirroring the assets list in crates/app/Cargo.toml's
  # [package.metadata.deb] so the app appears in launchers with its icon.
  postInstall = ''
    install -Dm644 assets/pain.desktop $out/share/applications/pain.desktop
    install -Dm644 assets/pain.svg $out/share/icons/hicolor/scalable/apps/pain.svg
    for size in 16 24 32 48 64 128 256 512; do
      install -Dm644 assets/pain-$size.png \
        $out/share/icons/hicolor/''${size}x''${size}/apps/pain.png
    done
  '';

  # Tests spin up PTYs and (in the render crate) wgpu contexts, which need
  # /dev/pts and a GPU — neither is reliable inside the Nix build sandbox.
  doCheck = false;

  meta = {
    description = "A cross-platform, multi-pane terminal emulator";
    homepage = "https://github.com/nwWagner/pain"; # TODO: set real repo URL
    license = lib.licenses.mit;
    mainProgram = "pain";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
