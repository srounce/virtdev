{
  description = "Persistent HID device proxy: keeps a virtual hidraw+evdev stand-in alive across disconnects";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?ref=nixos-unstable";
    blueprint.url = "github:numtide/blueprint";
    blueprint.inputs.nixpkgs.follows = "nixpkgs";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    crate2nix.url = "github:nix-community/crate2nix";
    crate2nix.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = inputs: inputs.blueprint {
    inherit inputs;
    systems = [ "x86_64-linux" "aarch64-linux" ];
    nixpkgs.overlays = [
      inputs.rust-overlay.overlays.default
      (final: _prev: {
        rustToolchain = final.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      })
    ];
  };
}
