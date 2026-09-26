# virtdev

Keeps a virtual copy of a HID device alive across disconnects. The copy is a
uhid device, so the kernel gives it both a hidraw node and evdev/js nodes with
the same VID/PID, name and report descriptor as the original. Games and
vendor tools keep one stable device to talk to while the real one reconnects.

Reports, output reports (rumble, LEDs) and GET/SET_REPORT requests are proxied
byte for byte. The real device is hidden from everything but the daemon so
programs never see two controllers.

## Quickstart: NixOS

1. Add the flake input and module:

   ```nix
   inputs.virtdev.url = "github:<you>/virtdev";
   # in your NixOS configuration
   imports = [ inputs.virtdev.nixosModules.virtdev ];
   ```

2. Capture the device identity once, with the device connected:

   ```
   sudo nix run github:<you>/virtdev -- inspect -f nix /dev/hidrawN > gt3wls.nix
   ```

   Find `N` with `ls -l /dev/input/by-id/*hidraw` for USB, or for Bluetooth
   `grep -l 'HID_ID=0005' /sys/class/hidraw/hidraw*/device/uevent`.

3. Enable the service, one entry per device:

   ```nix
   services.virtdev = {
     enable = true;
     devices.gt3wls.identity = import ./gt3wls.nix;
   };
   ```

4. Remove any custom udev rules for the device and any persistent-evdev
   setup, then rebuild. The virtual device exists from boot and the real one
   attaches whenever it connects.

## Quickstart: other distros

1. Build. Needs Rust, pkg-config and libudev headers (`libudev-dev` on
   Debian/Ubuntu, `systemd-devel` on Fedora):

   ```
   cargo build --release
   sudo install -m755 target/release/virtdev /usr/local/bin/
   ```

2. Create the service user and make sure uhid loads at boot:

   ```
   sudo useradd --system --no-create-home --shell /usr/sbin/nologin virtdev
   echo uhid | sudo tee /etc/modules-load.d/virtdev.conf
   sudo modprobe uhid
   ```

3. Capture the identity of each device while it is connected, and write
   the config:

   ```
   sudo mkdir -p /etc/virtdev
   sudo virtdev inspect -f toml /dev/hidrawN | sudo tee /etc/virtdev/gt3wls.toml
   ```

   `/etc/virtdev/config.toml`:

   ```toml
   [devices.gt3wls]
   identity_file = "/etc/virtdev/gt3wls.toml"
   ```

4. Generate and install the udev rules, then re-run them for anything
   already plugged in:

   ```
   sudo virtdev udev-rules /etc/virtdev/config.toml /etc/udev/rules.d --setfacl "$(command -v setfacl)"
   sudo udevadm control --reload
   sudo udevadm trigger --subsystem-match=misc --subsystem-match=hidraw --subsystem-match=input
   ```

   Rerun this step whenever you add a device to the config.

5. Install and start the service:

   ```
   sudo install -m644 contrib/virtdev.service /etc/systemd/system/
   sudo systemctl enable --now virtdev
   journalctl -u virtdev -f
   ```

## Config reference

```toml
[devices.gt3wls]
identity_file = "/etc/virtdev/gt3wls.toml"   # from `inspect -f toml`

[devices.ds4]
vendor = 0x054c       # required without an identity
product = 0x09cc
bus = "usb"           # optional: usb, bluetooth, i2c, or a number
uniq = "aa:bb:..."    # optional: serial or Bluetooth address
phys = "input1"       # optional: substring of the source phys, selects an interface
```

An identity can also be given inline as a `[devices.<name>.identity]` table.
With an identity the virtual device exists from daemon start and the match
keys default to the identity's values. Without one, the identity is learned
on first connect and cached under the cache directory for later boots.

Commands:

```
virtdev inspect [-f text|toml|nix|json] /dev/hidrawN
virtdev mirror /dev/hidrawN               # one-off clone, for testing
virtdev daemon config.toml [--cache-dir DIR]
virtdev udev-rules config.toml OUTDIR [--user U --group G --setfacl PATH]
```

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

## Development

```
nix develop
cargo build
crate2nix generate     # after changing Cargo.toml
nix flake check
```
