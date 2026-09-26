use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};

use crate::identity::StoredIdentity;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub devices: BTreeMap<String, DeviceConfig>,
}

/// One virtual device. Vendor and product are required; the rest narrow the
/// source match when several devices share an ID (multiple HID interfaces, or
/// two of the same controller).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    /// Match keys. Default to the identity's values when one is configured.
    pub vendor: Option<u16>,
    pub product: Option<u16>,
    #[serde(default, deserialize_with = "de_bus")]
    pub bus: Option<u16>,
    pub uniq: Option<String>,
    /// Substring of the source's HID phys string, e.g. "input1" to select an interface.
    pub phys: Option<String>,
    /// Identity to create the virtual device from before the source has ever
    /// been seen. Takes precedence over the cache.
    pub identity: Option<StoredIdentity>,
    /// Same as `identity`, read from a TOML file as written by `inspect --format toml`.
    pub identity_file: Option<PathBuf>,
}

impl DeviceConfig {
    pub fn matches(&self, bus: u16, vendor: u16, product: u16, uniq: &str, phys: &str) -> bool {
        self.vendor == Some(vendor)
            && self.product == Some(product)
            && self.bus.is_none_or(|b| b == bus)
            && self.uniq.as_deref().is_none_or(|u| u.eq_ignore_ascii_case(uniq))
            && self.phys.as_deref().is_none_or(|p| phys.contains(p))
    }

    pub fn configured_identity(&self) -> Result<Option<StoredIdentity>> {
        if let Some(id) = &self.identity {
            return Ok(Some(id.clone()));
        }
        let Some(p) = &self.identity_file else { return Ok(None) };
        let text = std::fs::read_to_string(p).with_context(|| format!("read {}", p.display()))?;
        toml::from_str(&text).map(Some).with_context(|| format!("parse {}", p.display()))
    }
}

pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let mut cfg: Config = toml::from_str(&text).with_context(|| format!("parse {}", path.display()))?;
    for (name, d) in cfg.devices.iter_mut() {
        if let Some(id) = d.configured_identity().with_context(|| format!("device {name}"))? {
            d.vendor.get_or_insert(id.vendor as u16);
            d.product.get_or_insert(id.product as u16);
            d.bus.get_or_insert(id.bus);
        }
        anyhow::ensure!(
            d.vendor.is_some() && d.product.is_some(),
            "device {name}: vendor and product are required without an identity"
        );
    }
    Ok(cfg)
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BusRepr {
    Num(u16),
    Name(String),
}

pub fn bus_from_name(s: &str) -> Option<u16> {
    Some(match s.to_ascii_lowercase().as_str() {
        "usb" => 0x03,
        "bluetooth" => 0x05,
        "virtual" => 0x06,
        "i2c" => 0x18,
        _ => return None,
    })
}

fn de_bus<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u16>, D::Error> {
    let v: Option<BusRepr> = Option::deserialize(d)?;
    Ok(match v {
        None => None,
        Some(BusRepr::Num(n)) => Some(n),
        Some(BusRepr::Name(s)) => {
            Some(bus_from_name(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown bus {s:?}")))?)
        }
    })
}
