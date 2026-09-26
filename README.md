# virtdev

Keeps a virtual copy of a HID device alive across disconnects. The copy is a
uhid device, so the kernel gives it both a hidraw node and evdev/js nodes with
the same VID/PID, name and report descriptor as the original. Games and
vendor tools keep a stable device to talk to while the real one reconnects.

Reports, output reports (rumble, LEDs) and GET/SET_REPORT requests are proxied
byte for byte. The virtual device is created from an on-disk cache at boot, so
it exists before the controller is powered on.

## Commands

```
virtdev inspect /dev/hidrawN            # identity and report descriptor
virtdev inspect --format toml /dev/hidrawN   # also nix, json
virtdev mirror  /dev/hidrawN            # one-off clone, useful for testing
virtdev daemon  config.toml             # run configured devices
virtdev udev-rules config.toml OUTDIR   # write 70- and 99-virtdev.rules
```

Config:

```toml
[devices.gt3wls]
vendor = 0x5411
product = 0x6969
bus = "bluetooth"     # optional: usb, bluetooth, i2c, or a number
uniq = "aa:bb:..."    # optional: serial or Bluetooth address
phys = "input1"       # optional: substring of the source phys, selects an interface
identity_file = "/etc/virtdev/gt3wls.toml"   # optional: from `inspect --format toml`
```

With `identity_file` (or an inline `[devices.<name>.identity]` table) the
virtual device exists from daemon start. Without it, the identity is learned
on first connect and cached under the cache directory for later boots.

## udev rules

Some rules are unavoidable: the daemon needs `/dev/uhid` and the source nodes,
and the only way to hide a HID device from other programs without losing its
hidraw is to take away their permission to open it. `virtdev udev-rules`
writes two files. 70-virtdev.rules hands the daemon its devices and tags the
proxy hidraw with uaccess. 99-virtdev.rules strips uaccess from the sources,
sets them 0600 owned by the daemon user, and clears any ACL the uaccess
builtin already applied. The daemon repeats the chmod and ACL removal on every
attach as a fallback. Because the proxy shares its source's VID/PID, only the
phys string (`virtdev:<name>`) tells them apart.

## NixOS

```nix
{
  imports = [ virtdev.nixosModules.virtdev ];
  services.virtdev = {
    enable = true;
    devices.gt3wls = {
      vendor = "5411"; product = "6969"; bus = "bluetooth";
      identityFile = ./gt3wls.toml;
    };
  };
}
```

The module runs the daemon as an unprivileged `virtdev` user and installs the
rules from `virtdev udev-rules`. Cached identities live in `/var/cache/virtdev`.

## Development

```
nix develop
cargo build
crate2nix generate     # after changing Cargo.toml
```
