//! On-disk copy of a source device's identity, so the virtual device can be
//! created at boot before the physical one has ever connected.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::identity::StoredIdentity;
use crate::uhid::Identity;

pub fn path(dir: &Path, device: &str) -> PathBuf {
    dir.join(format!("{device}.toml"))
}

pub fn load(dir: &Path, device: &str, phys: &str) -> Result<Option<Identity>> {
    let p = path(dir, device);
    if !p.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    let s: StoredIdentity = toml::from_str(&text).with_context(|| format!("parse {}", p.display()))?;
    s.to_identity(phys).map(Some).with_context(|| p.display().to_string())
}

pub fn save(dir: &Path, device: &str, id: &Identity) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let p = path(dir, device);
    let tmp = p.with_extension("toml.tmp");
    std::fs::write(&tmp, toml::to_string(&StoredIdentity::from_identity(id))?)
        .with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, &p).with_context(|| format!("rename to {}", p.display()))?;
    Ok(())
}
