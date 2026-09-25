{ pkgs, perSystem, ... }:
pkgs.mkShell {
  packages = [
    pkgs.rustToolchain
    perSystem.crate2nix.default
    pkgs.pkg-config
    pkgs.udev
  ];
  RUST_SRC_PATH = "${pkgs.rustToolchain}/lib/rustlib/src/rust/library";
}
