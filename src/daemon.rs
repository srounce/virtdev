//! Long-running mode: watch udev for configured source devices, keep one
//! virtual device per config entry alive, and attach/detach sources as they
//! come and go.

use std::collections::BTreeMap;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use log::{error, info, warn};

use crate::cache;
use crate::config::{Config, DeviceMatch};
use crate::hidraw::Hidraw;
use crate::proxy::{Ctrl, Handle, Proxy};
use crate::uhid::{Identity, Uhid};

struct Managed {
    matcher: DeviceMatch,
    handle: Option<Handle>,
    /// devnode of the currently attached source, used to recognise its removal.
    source: Option<PathBuf>,
}

pub fn phys_for(name: &str) -> String {
    format!("virtdev:{name}")
}

pub fn run(config: Config, cache_dir: &Path) -> Result<()> {
    let mut managed: BTreeMap<String, Managed> = BTreeMap::new();
    for (name, matcher) in config.devices {
        let mut m = Managed { matcher, handle: None, source: None };
        match cache::load(cache_dir, &name, &phys_for(&name)) {
            Ok(Some(ident)) => {
                info!("{name}: creating virtual device from cache ({})", ident.name);
                m.handle = Some(spawn_proxy(&name, &ident)?);
            }
            Ok(None) => info!("{name}: no cache, waiting for source"),
            Err(e) => warn!("{name}: ignoring bad cache: {e:#}"),
        }
        managed.insert(name, m);
    }

    let monitor = udev::MonitorBuilder::new()?
        .match_subsystem("hidraw")?
        .listen()
        .context("udev monitor")?;

    let mut en = udev::Enumerator::new()?;
    en.match_subsystem("hidraw")?;
    for dev in en.scan_devices()? {
        on_add(&dev, &mut managed, cache_dir);
    }

    loop {
        let mut fds = [libc::pollfd { fd: monitor.as_raw_fd(), events: libc::POLLIN, revents: 0 }];
        // SAFETY: fds is a valid single-element pollfd array.
        if unsafe { libc::poll(fds.as_mut_ptr(), 1, -1) } < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e).context("poll udev monitor");
        }
        for ev in monitor.iter() {
            match ev.event_type() {
                udev::EventType::Add => on_add(&ev.device(), &mut managed, cache_dir),
                udev::EventType::Remove => on_remove(&ev.device(), &mut managed),
                _ => {}
            }
        }
    }
}

struct SourceInfo {
    bus: u16,
    vendor: u16,
    product: u16,
    uniq: String,
    phys: String,
    version: u32,
}

/// Reads identity from udev properties so that no hidraw node has to be
/// opened until it is known to be one we want.
fn source_info(dev: &udev::Device) -> Option<SourceInfo> {
    let hid = dev.parent_with_subsystem("hid").ok().flatten()?;
    let prop = |k: &str| hid.property_value(k).map(|v| v.to_string_lossy().into_owned()).unwrap_or_default();
    let id = prop("HID_ID");
    let mut parts = id.split(':').map(|p| u32::from_str_radix(p, 16).ok());
    let (bus, vendor, product) = (parts.next()??, parts.next()??, parts.next()??);

    // The HID core does not expose the device version, but its input child does.
    let mut version = 0;
    if let Ok(mut en) = udev::Enumerator::new() {
        let _ = en.match_subsystem("input");
        let _ = en.match_parent(&hid);
        if let Ok(devs) = en.scan_devices() {
            for d in devs {
                if let Some(p) = d.property_value("PRODUCT") {
                    if let Some(v) = p.to_string_lossy().split('/').nth(3) {
                        if let Ok(v) = u32::from_str_radix(v, 16) {
                            version = v;
                            break;
                        }
                    }
                }
            }
        }
    }

    Some(SourceInfo {
        bus: bus as u16,
        vendor: vendor as u16,
        product: product as u16,
        uniq: prop("HID_UNIQ"),
        phys: prop("HID_PHYS"),
        version,
    })
}

fn on_add(dev: &udev::Device, managed: &mut BTreeMap<String, Managed>, cache_dir: &Path) {
    let Some(node) = dev.devnode().map(Path::to_path_buf) else { return };
    let Some(si) = source_info(dev) else { return };
    if si.phys.starts_with("virtdev:") {
        return;
    }
    for (name, m) in managed.iter_mut() {
        if m.source.is_some() || !m.matcher.matches(si.bus, si.vendor, si.product, &si.uniq, &si.phys) {
            continue;
        }
        if let Err(e) = attach(name, m, &node, &si, cache_dir) {
            error!("{name}: attach {} failed: {e:#}", node.display());
        }
        return;
    }
}

fn attach(name: &str, m: &mut Managed, node: &Path, si: &SourceInfo, cache_dir: &Path) -> Result<()> {
    let src = Hidraw::open(node)?;
    let info = src.info()?;
    let ident = Identity {
        name: info.name.clone(),
        phys: phys_for(name),
        uniq: info.uniq.clone(),
        bus: info.bus,
        vendor: info.vendor as u32,
        product: info.product as u32,
        version: si.version,
        country: 0,
        descriptor: info.descriptor.clone(),
    };

    let stale = match cache::load(cache_dir, name, &ident.phys)? {
        Some(c) => c.descriptor != ident.descriptor || c.bus != ident.bus || c.uniq != ident.uniq,
        None => true,
    };
    if stale {
        cache::save(cache_dir, name, &ident)?;
        if m.handle.is_some() {
            warn!("{name}: source identity changed, recreating virtual device");
            if let Some(h) = m.handle.take() {
                h.send(Ctrl::Shutdown);
            }
        }
    }
    if m.handle.is_none() {
        info!("{name}: creating virtual device from {} ({})", node.display(), info.name);
        m.handle = Some(spawn_proxy(name, &ident)?);
    }

    info!("{name}: attaching {}", node.display());
    m.handle.as_ref().unwrap().send(Ctrl::Attach(src));
    m.source = Some(node.to_path_buf());
    Ok(())
}

fn on_remove(dev: &udev::Device, managed: &mut BTreeMap<String, Managed>) {
    let Some(node) = dev.devnode() else { return };
    for (name, m) in managed.iter_mut() {
        if m.source.as_deref() == Some(node) {
            info!("{name}: source {} removed", node.display());
            m.source = None;
            if let Some(h) = &m.handle {
                h.send(Ctrl::Detach);
            }
        }
    }
}

fn spawn_proxy(name: &str, ident: &Identity) -> Result<Handle> {
    let uhid = Uhid::create(ident).with_context(|| format!("{name}: create uhid device"))?;
    let (proxy, handle) = Proxy::new(name.to_string(), uhid)?;
    let n = name.to_string();
    std::thread::Builder::new().name(n.clone()).spawn(move || {
        if let Err(e) = proxy.run() {
            error!("{n}: proxy exited: {e:#}");
        }
    })?;
    Ok(handle)
}
