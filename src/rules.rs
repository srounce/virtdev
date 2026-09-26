//! udev rule generation. Two files are needed because the uaccess tag must be
//! set before 73-seat-late.rules, while hiding must run after every rule that
//! might grant access to the source.

use std::fmt::Write;

use crate::config::Config;

pub struct Options<'a> {
    pub user: &'a str,
    pub group: &'a str,
    pub setfacl: &'a str,
}

/// Grants the daemon /dev/uhid, gives the logged-in user the proxy nodes, and
/// names them under /dev/input/by-id. The standard by-id rules skip uhid
/// devices because they have no USB parent to derive a serial from.
pub fn access(cfg: &Config, o: &Options) -> String {
    let mut out = format!(
        "SUBSYSTEM==\"misc\", KERNEL==\"uhid\", OWNER=\"{}\", GROUP=\"{}\", MODE=\"0600\"\n\
         SUBSYSTEM==\"hidraw\", IMPORT{{parent}}=\"HID_PHYS\"\n\
         SUBSYSTEM==\"hidraw\", ENV{{HID_PHYS}}==\"virtdev:*\", TAG+=\"uaccess\"\n\
         SUBSYSTEM==\"input\", ATTRS{{phys}}==\"virtdev:*\", TAG+=\"uaccess\"\n",
        o.user, o.group
    );
    for name in cfg.devices.keys() {
        let _ = writeln!(out, "# virtdev: by-id links for \"{name}\"");
        let _ = writeln!(
            out,
            "SUBSYSTEM==\"hidraw\", ENV{{HID_PHYS}}==\"virtdev:{name}\", SYMLINK+=\"input/by-id/virtdev-{name}-hidraw\""
        );
        let ev = format!("SUBSYSTEM==\"input\", KERNEL==\"event*\", ATTRS{{phys}}==\"virtdev:{name}\"");
        for (prop, suffix) in [("ID_INPUT_JOYSTICK", "joystick"), ("ID_INPUT_KEYBOARD", "kbd"), ("ID_INPUT_MOUSE", "mouse")] {
            let _ = writeln!(out, "{ev}, ENV{{{prop}}}==\"1\", SYMLINK+=\"input/by-id/virtdev-{name}-event-{suffix}\"");
        }
        let _ = writeln!(
            out,
            "{ev}, ENV{{ID_INPUT_JOYSTICK}}!=\"1\", ENV{{ID_INPUT_KEYBOARD}}!=\"1\", ENV{{ID_INPUT_MOUSE}}!=\"1\", SYMLINK+=\"input/by-id/virtdev-{name}-event\""
        );
        let _ = writeln!(
            out,
            "SUBSYSTEM==\"input\", KERNEL==\"js*\", ATTRS{{phys}}==\"virtdev:{name}\", SYMLINK+=\"input/by-id/virtdev-{name}-joystick\""
        );
    }
    out
}

/// Makes each source device unreachable for anything but the daemon. The proxy
/// shares the source's VID/PID and is told apart by its phys prefix. The
/// uaccess builtin is a RUN entry that applies its ACL after MODE, so the ACL
/// is stripped by a later RUN.
pub fn hide(cfg: &Config, o: &Options) -> String {
    let mut out = String::from("SUBSYSTEM==\"input\", ATTRS{phys}==\"virtdev:*\", ENV{VIRTDEV_PROXY}=\"1\"\n");
    let restrict = format!(
        "TAG-=\"uaccess\", OWNER=\"{}\", GROUP=\"{}\", MODE=\"0600\", RUN+=\"{} -b $devnode\"",
        o.user, o.group, o.setfacl
    );
    for (name, d) in &cfg.devices {
        let bus = d.bus.map_or("*".to_string(), |b| format!("{b:04X}"));
        let kernels = format!("{bus}:{:04X}:{:04X}.*", d.vendor.unwrap_or(0), d.product.unwrap_or(0));
        let _ = writeln!(out, "# virtdev: hide source of \"{name}\"");
        let _ = writeln!(out, "SUBSYSTEM==\"hidraw\", KERNELS==\"{kernels}\", IMPORT{{parent}}=\"HID_PHYS\"");
        let _ = writeln!(out, "SUBSYSTEM==\"hidraw\", KERNELS==\"{kernels}\", ENV{{HID_PHYS}}!=\"virtdev:*\", {restrict}");
        let _ = writeln!(
            out,
            "SUBSYSTEM==\"input\", ENV{{VIRTDEV_PROXY}}!=\"1\", KERNELS==\"{kernels}\", \
             ENV{{ID_INPUT}}=\"\", ENV{{ID_INPUT_JOYSTICK}}=\"\", ENV{{ID_INPUT_KEYBOARD}}=\"\", \
             ENV{{ID_INPUT_MOUSE}}=\"\", ENV{{ID_INPUT_TABLET}}=\"\", ENV{{ID_INPUT_TOUCHPAD}}=\"\", \
             ENV{{LIBINPUT_IGNORE_DEVICE}}=\"1\", {restrict}"
        );
    }
    out
}
