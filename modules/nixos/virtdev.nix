{ flake, ... }:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.virtdev;
  fmt = pkgs.formats.toml { };

  busNumber = {
    usb = 3;
    bluetooth = 5;
    virtual = 6;
    i2c = 24;
  };
  idType = lib.types.either lib.types.ints.u16 (lib.types.strMatching "[0-9a-fA-F]{1,4}");
  toId = v: if builtins.isString v then lib.fromHexString v else v;
  hex4 = n: lib.toUpper (lib.fixedWidthString 4 "0" (lib.toHexString (toId n)));
  busPattern = bus:
    if bus == null then "*"
    else hex4 (if builtins.isString bus then busNumber.${bus} else bus);

  # Kernel name of the HID device behind a source: <bus>:<vid>:<pid>.<instance>.
  kernels = d: "${busPattern d.bus}:${hex4 d.vendor}:${hex4 d.product}.*";

  configFile = fmt.generate "virtdev.toml" {
    devices = lib.mapAttrs (_: d: lib.filterAttrs (_: v: v != null) (d // { vendor = toId d.vendor; product = toId d.product; })) cfg.devices;
  };

  # The proxy device carries the same VID/PID as its source. It is told apart
  # by the phys string the daemon sets, so the source alone loses its access.
  # Runs at 99 so it overrides any earlier rule granting access to the source.
  hideRules = lib.concatStrings (lib.mapAttrsToList (name: d: ''
    # virtdev: hide source of "${name}"
    SUBSYSTEM=="hidraw", KERNELS=="${kernels d}", IMPORT{parent}="HID_PHYS"
    SUBSYSTEM=="hidraw", KERNELS=="${kernels d}", ENV{HID_PHYS}!="virtdev:*", OWNER="${cfg.user}", GROUP="${cfg.group}", MODE="0600"
    SUBSYSTEM=="input", ATTRS{phys}=="virtdev:*", ENV{VIRTDEV_PROXY}="1"
    SUBSYSTEM=="input", ENV{VIRTDEV_PROXY}!="1", KERNELS=="${kernels d}", ENV{ID_INPUT}="", ENV{ID_INPUT_JOYSTICK}="", ENV{ID_INPUT_KEYBOARD}="", ENV{ID_INPUT_MOUSE}="", ENV{ID_INPUT_TABLET}="", ENV{ID_INPUT_TOUCHPAD}="", ENV{LIBINPUT_IGNORE_DEVICE}="1", OWNER="${cfg.user}", GROUP="${cfg.group}", MODE="0600"
  '') cfg.devices);

  # Access grants sit before 73-seat-late.rules so the uaccess tag takes effect.
  accessRules = ''
    SUBSYSTEM=="misc", KERNEL=="uhid", OWNER="${cfg.user}", GROUP="${cfg.group}", MODE="0600"
  '' + lib.optionalString cfg.proxyAccess ''
    SUBSYSTEM=="hidraw", IMPORT{parent}="HID_PHYS"
    SUBSYSTEM=="hidraw", ENV{HID_PHYS}=="virtdev:*", TAG+="uaccess"
  '';

  rulesPackage = pkgs.runCommand "virtdev-udev-rules" {
    inherit accessRules;
    hideRules = lib.optionalString cfg.hideSources hideRules;
    passAsFile = [ "accessRules" "hideRules" ];
  } ''
    mkdir -p $out/lib/udev/rules.d
    cp "$accessRulesPath" $out/lib/udev/rules.d/70-virtdev.rules
    cp "$hideRulesPath" $out/lib/udev/rules.d/99-virtdev.rules
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
        type = lib.types.nullOr (lib.types.either (lib.types.enum (lib.attrNames busNumber)) lib.types.ints.u16);
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
          gt3wls = { vendor = "5411"; product = "6969"; bus = "bluetooth"; };
        }
      '';
    };

    proxyAccess = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Tag the proxy hidraw nodes with uaccess so the logged-in user can open them.";
    };

    hideSources = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Install udev rules so only the daemon can open the source devices.";
    };

    udevRules = lib.mkOption {
      type = lib.types.package;
      readOnly = true;
      default = rulesPackage;
      description = "Generated udev rules package.";
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
        DeviceAllow = [ "/dev/uhid rw" "char-hidraw rw" ];
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
