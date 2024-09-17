{pkgs ? import <nixpkgs> {}}:
with pkgs; let
  deps = [
    openssl
    pkg-config
  ];
in
  mkShell {
    buildInputs = deps;
    LD_LIBRARY_PATH = lib.makeLibraryPath deps;
  }
