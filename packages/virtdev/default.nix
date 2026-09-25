{ pkgs, ... }:
let
  cargoNix = pkgs.callPackage ../../Cargo.nix {
    buildRustCrateForPkgs = p: p.buildRustCrate.override {
      rustc = p.rustToolchain;
      cargo = p.rustToolchain;
    };
  };
in
cargoNix.rootCrate.build
