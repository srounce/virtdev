# Evaluates the NixOS module with a sample device and materialises the
# generated config and udev rules so regressions in either show up in
# `nix flake check`.
{ flake, inputs, pkgs, ... }:
let
  eval = inputs.nixpkgs.lib.nixosSystem {
    inherit (pkgs.stdenv.hostPlatform) system;
    modules = [
      flake.nixosModules.virtdev
      {
        boot.loader.grub.enable = false;
        fileSystems."/" = { device = "none"; fsType = "tmpfs"; };
        system.stateVersion = "25.11";
        services.virtdev = {
          enable = true;
          devices.gt3wls = { vendor = "5411"; product = "6969"; bus = "bluetooth"; };
          devices.ds4 = { vendor = 1356; product = "09cc"; };
        };
      }
    ];
  };
  cfg = eval.config;
in
pkgs.runCommand "virtdev-nixos-module" { } ''
  mkdir -p $out
  cp ${cfg.services.virtdev.configFile} $out/virtdev.toml
  cp ${cfg.services.virtdev.udevRules}/lib/udev/rules.d/*.rules $out/
  grep -q 'ENV{HID_PHYS}=="virtdev:\*", TAG+="uaccess"' $out/70-virtdev.rules
  grep -q 'KERNELS=="0005:5411:6969.\*"' $out/99-virtdev.rules
  grep -q 'KERNELS=="\*:054C:09CC.\*"' $out/99-virtdev.rules
  grep -q '^\[devices.gt3wls\]' $out/virtdev.toml
  grep -q 'bus = "bluetooth"' $out/virtdev.toml
  ! grep -q 'uniq' $out/virtdev.toml
''
