{ pkgs, ... }:
let
  cargoToml = builtins.fromTOML (builtins.readFile ../../Cargo.toml);
  cargoNix = pkgs.callPackage ../../Cargo.nix {
    buildRustCrateForPkgs = p: p.buildRustCrate.override {
      rustc = p.rustToolchain;
      cargo = p.rustToolchain;
    };
  };
in
# buildRustCrate names every crate rust_<name>; give the binary its own name.
cargoNix.rootCrate.build.overrideAttrs (old: {
  pname = cargoToml.package.name;
  inherit (cargoToml.package) version;
  name = "${cargoToml.package.name}-${cargoToml.package.version}";
  # The version comes from Cargo.toml along with the source, not from a bump.
  __intentionallyOverridingVersion = true;
  meta = (old.meta or { }) // {
    description = cargoToml.package.description;
    license = pkgs.lib.licenses.gpl2Only;
    mainProgram = "virtdev";
  };
})
