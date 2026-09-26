{ flake, ... }:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.virtdev;
  fmt = pkgs.formats.toml { };

  idType = lib.types.either lib.types.ints.u16 (lib.types.strMatching "[0-9a-fA-F]{1,4}");
  toId = v: if builtins.isString v then lib.fromHexString v else v;

  configFile = fmt.generate "virtdev.toml" {
    devices = lib.mapAttrs (_: d:
      lib.filterAttrs (_: v: v != null) {
        inherit (d) bus uniq phys identity;
        vendor = toId d.vendor;
        product = toId d.product;
        identity_file = d.identityFile;
      }) cfg.devices;
  };

  rulesPackage = pkgs.runCommand "virtdev-udev-rules" { } ''
    ${cfg.package}/bin/virtdev udev-rules ${configFile} $out/lib/udev/rules.d \
      --user ${cfg.user} --group ${cfg.group} --setfacl ${pkgs.acl}/bin/setfacl
    ${lib.optionalString (!cfg.hideSources) "rm $out/lib/udev/rules.d/99-virtdev.rules"}
  '';

  deviceModule = { ... }: {
    options = {
      vendor = lib.mkOption {
        type = idType;
        description = "Vendor ID of the source device, as an integer or hex string.";
      };
      product = lib.mkOption {
        type = idType;
        description = "Product ID of the source device, as an integer or hex string.";
      };
      bus = lib.mkOption {
        type = lib.types.nullOr (lib.types.either (lib.types.enum [ "usb" "bluetooth" "virtual" "i2c" ]) lib.types.ints.u16);
        default = null;
        description = "Restrict to a HID bus type. Null matches any.";
      };
      uniq = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Restrict to a serial / Bluetooth address. Not applied to the udev hide rules.";
      };
      phys = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        description = "Substring of the source phys string, e.g. \"input1\" to select one HID interface.";
      };
      identity = lib.mkOption {
        type = lib.types.nullOr (lib.types.attrsOf (lib.types.either lib.types.str lib.types.int));
        default = null;
        description = "Inline device identity as printed by `virtdev inspect --toml`, so the virtual device exists before the source has ever connected.";
      };
      identityFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "Path to a TOML identity file as written by `virtdev inspect --toml`.";
      };
    };
  };
in
{
  options.services.virtdev = {
    enable = lib.mkEnableOption "virtdev persistent HID proxy";

    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.stdenv.hostPlatform.system}.virtdev;
      defaultText = lib.literalExpression "virtdev.packages.\${system}.virtdev";
    };

    devices = lib.mkOption {
      type = lib.types.attrsOf (lib.types.submodule deviceModule);
      default = { };
      description = "Virtual devices to keep alive, keyed by a short name.";
      example = lib.literalExpression ''
        {
          gt3wls = {
            vendor = "5411"; product = "6969"; bus = "bluetooth";
            identityFile = ./gt3wls.toml;
          };
        }
      '';
    };

    hideSources = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install the generated 99-virtdev.rules so only the daemon can open the source devices.";
    };

    udevRules = lib.mkOption {
      type = lib.types.package;
      readOnly = true;
      default = rulesPackage;
      description = "Rules package produced by `virtdev udev-rules`.";
    };

    configFile = lib.mkOption {
      type = lib.types.path;
      readOnly = true;
      default = configFile;
      description = "Generated daemon configuration.";
    };

    logLevel = lib.mkOption {
      type = lib.types.str;
      default = "info";
      description = "RUST_LOG filter for the daemon.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "virtdev";
    };
    group = lib.mkOption {
      type = lib.types.str;
      default = "virtdev";
    };
  };

  config = lib.mkIf cfg.enable {
    boot.kernelModules = [ "uhid" ];

    users.users.${cfg.user} = {
      isSystemUser = true;
      group = cfg.group;
    };
    users.groups.${cfg.group} = { };

    services.udev.packages = [ rulesPackage ];

    systemd.services.virtdev = {
      description = "Persistent HID device proxy";
      wantedBy = [ "multi-user.target" ];
      after = [ "systemd-udevd.service" ];
      environment.RUST_LOG = cfg.logLevel;
      serviceConfig = {
        ExecStart = "${cfg.package}/bin/virtdev daemon ${cfg.configFile}";
        User = cfg.user;
        Group = cfg.group;
        CacheDirectory = "virtdev";
        Restart = "on-failure";
        RestartSec = 2;

        DevicePolicy = "closed";
        DeviceAllow = [ "/dev/uhid rw" "char-hidraw rw" "char-input rw" ];
        RestrictAddressFamilies = [ "AF_NETLINK" "AF_UNIX" ];
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictRealtime = true;
        RestrictNamespaces = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" ];
        CapabilityBoundingSet = "";
      };
    };
  };
}
