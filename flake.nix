{
  description = "A minimal, keyboard-driven PDF viewer";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        yavcpdf = pkgs.callPackage ./packaging/nix/package.nix { };
        default = yavcpdf;
      });

      overlays.default = final: _prev: {
        yavcpdf = final.callPackage ./packaging/nix/package.nix { };
      };

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.yavcpdf ];
          packages = with pkgs; [
            clippy
            rustfmt
            rust-analyzer
          ];
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (
            with pkgs;
            [
              libGL
              libxkbcommon
              wayland
              libx11
              libxcursor
              libxi
              libxrender
              dbus
            ]
          );
        };
      });
    };
}
