use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub devices: BTreeMap<String, DeviceMatch>,
}

/// Which source hidraw a virtual device tracks. Vendor and product are
/// required; the rest narrow the match when several devices share an ID
/// (multiple HID interfaces, or two of the same controller).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceMatch {
    pub vendor: u16,
    pub product: u16,
    #[serde(default, deserialize_with = "de_bus")]
    pub bus: Option<u16>,
    pub uniq: Option<String>,
    /// Substring of the source's HID phys string, e.g. "input1" to select an interface.
    pub phys: Option<String>,
}

impl DeviceMatch {
    pub fn matches(&self, bus: u16, vendor: u16, product: u16, uniq: &str, phys: &str) -> bool {
        self.vendor == vendor
            && self.product == product
            && self.bus.is_none_or(|b| b == bus)
            && self.uniq.as_deref().is_none_or(|u| u.eq_ignore_ascii_case(uniq))
            && self.phys.as_deref().is_none_or(|p| phys.contains(p))
    }
}

pub fn load(path: &Path) -> Result<Config> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

#[derive(Deserialize)]
#[serde(untagged)]
enum BusRepr {
    Num(u16),
    Name(String),
}

fn de_bus<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u16>, D::Error> {
    let v: Option<BusRepr> = Option::deserialize(d)?;
    Ok(match v {
        None => None,
        Some(BusRepr::Num(n)) => Some(n),
        Some(BusRepr::Name(s)) => Some(match s.to_ascii_lowercase().as_str() {
            "usb" => 0x03,
            "bluetooth" => 0x05,
            "virtual" => 0x06,
            "i2c" => 0x18,
            other => return Err(serde::de::Error::custom(format!("unknown bus {other:?}"))),
        }),
    })
}
