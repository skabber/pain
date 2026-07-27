{
  description = "pain — a cross-platform, multi-pane terminal emulator";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs [
          "x86_64-linux"
          "aarch64-linux"
        ] (system: f nixpkgs.legacyPackages.${system});
    in
    {
      # `nix build` / usable as a flake input from a dotfiles config:
      #   inputs.pain.url = "github:<owner>/pain";
      #   environment.systemPackages = [ inputs.pain.packages.x86_64-linux.default ];
      packages = forAllSystems (pkgs: {
        default = pkgs.callPackage ./package.nix { src = self; };
      });

      devShells = forAllSystems (
        pkgs:
        let
          # winit/wgpu/arboard don't link these at build time — they dlopen()
          # them at runtime (see the `depends` line in crates/app/Cargo.toml).
          # On NixOS a bare binary can't find them, so expose them via
          # LD_LIBRARY_PATH inside the shell.
          runtimeLibs = with pkgs; [
            vulkan-loader # libvulkan
            libglvnd # libEGL
            wayland # libwayland-client / libwayland-egl
            libxkbcommon
            xorg.libX11 # libX11 + libX11-xcb
            xorg.libXcursor
            xorg.libXi
            xorg.libxcb
          ];
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              # Rust toolchain (edition 2024 needs rustc >= 1.85)
              rustc
              cargo
              rustfmt
              clippy
              rust-analyzer

              # wayland-sys finds libwayland via pkg-config at build time
              pkg-config
            ];

            buildInputs = with pkgs; [
              wayland
              wayland-scanner
              libxkbcommon
            ];

            RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";

            shellHook = ''
              export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath runtimeLibs}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
            '';
          };
        }
      );
    };
}
