//! On-disk copy of a source device's identity, so the virtual device can be
//! created at boot before the physical one has ever connected.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::uhid::Identity;

#[derive(Serialize, Deserialize)]
struct Stored {
    name: String,
    uniq: String,
    bus: u16,
    vendor: u32,
    product: u32,
    version: u32,
    country: u32,
    descriptor: String,
}

pub fn path(dir: &Path, device: &str) -> PathBuf {
    dir.join(format!("{device}.toml"))
}

pub fn load(dir: &Path, device: &str, phys: &str) -> Result<Option<Identity>> {
    let p = path(dir, device);
    if !p.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    let s: Stored = toml::from_str(&text).with_context(|| format!("parse {}", p.display()))?;
    let descriptor = unhex(&s.descriptor).with_context(|| format!("descriptor in {}", p.display()))?;
    Ok(Some(Identity {
        name: s.name,
        phys: phys.to_string(),
        uniq: s.uniq,
        bus: s.bus,
        vendor: s.vendor,
        product: s.product,
        version: s.version,
        country: s.country,
        descriptor,
    }))
}

pub fn save(dir: &Path, device: &str, id: &Identity) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let s = Stored {
        name: id.name.clone(),
        uniq: id.uniq.clone(),
        bus: id.bus,
        vendor: id.vendor,
        product: id.product,
        version: id.version,
        country: id.country,
        descriptor: id.descriptor.iter().map(|b| format!("{b:02x}")).collect(),
    };
    let p = path(dir, device);
    let tmp = p.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string(&s)?).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &p).with_context(|| format!("rename to {}", p.display()))?;
    Ok(())
}

fn unhex(s: &str) -> Result<Vec<u8>> {
    anyhow::ensure!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(Into::into))
        .collect()
}
