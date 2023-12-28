{
  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";


  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forEachSupportedSystem = f: nixpkgs.lib.genAttrs supportedSystems (system: f {
        pkgs = import nixpkgs { inherit system; };
      });
    in
    {
      devShells = forEachSupportedSystem ({ pkgs }: {
        default = pkgs.mkShell rec {
          packages = with pkgs; [
            rustc
            cargo
            rustfmt

            bacon
            cargo-deny
            cargo-edit
            cargo-watch
            rust-analyzer
          ];

          buildInputs = with pkgs; [
            xorg.libX11
            xorg.libXcursor
            xorg.libXrandr
            xorg.libXi
            libxkbcommon
            libGL
            fontconfig
          ];

          LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath buildInputs}";
        };
      });
    };
}
