{ pkgs, ... }:
let
  cargoNix = pkgs.callPackage ../../Cargo.nix {
    buildRustCrateForPkgs = p: p.buildRustCrate.override {
      rustc = p.rustToolchain;
      cargo = p.rustToolchain;
    };
  };
in
cargoNix.rootCrate.build // {
  meta.license = pkgs.lib.licenses.gpl2Only;
  meta.mainProgram = "virtdev";
  meta.description = "Persistent HID device proxy over uhid";
}
