{
  inputs.astapkgs.url = "github:Astavie/astapkgs";

  outputs = { self, astapkgs }: astapkgs.lib.package {

    # package = pkgs: with pkgs; ...

    devShell = pkgs: with pkgs; mkShell {

      buildInputs = [
        dev.rust-nightly
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

  } [ "x86_64-linux" ];
}
