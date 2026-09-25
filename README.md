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
virtdev inspect /dev/hidrawN          # identity and report descriptor
virtdev mirror  /dev/hidrawN          # one-off clone, useful for testing
virtdev daemon  config.toml           # run configured devices
```

Config:

```toml
[devices.gt3wls]
vendor = 0x5411
product = 0x6969
bus = "bluetooth"     # optional: usb, bluetooth, i2c, or a number
uniq = "aa:bb:..."    # optional: serial or Bluetooth address
phys = "input1"       # optional: substring of the source phys, selects an interface
```

## NixOS

```nix
{
  imports = [ virtdev.nixosModules.virtdev ];
  services.virtdev = {
    enable = true;
    devices.gt3wls = { vendor = "5411"; product = "6969"; bus = "bluetooth"; };
  };
}
```

The module runs the daemon as an unprivileged `virtdev` user and installs udev
rules that give it `/dev/uhid` and the source devices, hide the sources from
everyone else (`hideSources`), and grant the logged-in user the proxy's hidraw
node (`proxyAccess`). Cached identities live in `/var/cache/virtdev`.

Because the proxy shares its source's VID/PID, only the phys string
(`virtdev:<name>`) tells them apart. Any custom udev rule for the device
should match on that if it needs to distinguish the two.

## Development

```
nix develop
cargo build
crate2nix generate     # after changing Cargo.toml
```
