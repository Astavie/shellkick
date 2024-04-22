{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.rust-overlay.url = "github:oxalica/rust-overlay";
  inputs.rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

  outputs = { self, nixpkgs, rust-overlay }: 
    let
      overlays = [
        rust-overlay.overlays.default
        (final: prev: {
          rustToolchain = prev.rust-bin.nightly.latest.default.override { extensions = [ "rust-src" ]; };
        })
      ];
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forEachSupportedSystem = f: nixpkgs.lib.genAttrs supportedSystems (system: f {
        pkgs = import nixpkgs { inherit overlays system; };
      });
    in
    {
      devShells = forEachSupportedSystem ({ pkgs }: with pkgs; {
        default = pkgs.mkShell {

          buildInputs = [
            rustToolchain
          ];

          LD_LIBRARY_PATH = lib.makeLibraryPath [
            xorg.libX11
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
            libxkbcommon
            libGL
            fontconfig
          ];

        };
      });
    };
}
