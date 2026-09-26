//! Serialisable form of a device identity, shared by the cache and by
//! identities embedded in the config.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::uhid::Identity;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredIdentity {
    pub name: String,
    #[serde(default)]
    pub uniq: String,
    pub bus: u16,
    pub vendor: u32,
    pub product: u32,
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub country: u32,
    /// Report descriptor as a hex string.
    pub descriptor: String,
}

impl StoredIdentity {
    pub fn from_identity(id: &Identity) -> Self {
        Self {
            name: id.name.clone(),
            uniq: id.uniq.clone(),
            bus: id.bus,
            vendor: id.vendor,
            product: id.product,
            version: id.version,
            country: id.country,
            descriptor: id.descriptor.iter().map(|b| format!("{b:02x}")).collect(),
        }
    }

    pub fn to_identity(&self, phys: &str) -> Result<Identity> {
        Ok(Identity {
            name: self.name.clone(),
            phys: phys.to_string(),
            uniq: self.uniq.clone(),
            bus: self.bus,
            vendor: self.vendor,
            product: self.product,
            version: self.version,
            country: self.country,
            descriptor: unhex(&self.descriptor).context("descriptor")?,
        })
    }
}

fn unhex(s: &str) -> Result<Vec<u8>> {
    let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    anyhow::ensure!(s.len().is_multiple_of(2), "odd hex length");
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(Into::into))
        .collect()
}

impl StoredIdentity {
    /// Nix attribute set literal, for pasting into a NixOS `identity` option.
    pub fn to_nix(&self) -> String {
        let s = |v: &str| format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"").replace("${", "\\${"));
        format!(
            "{{\n  name = {};\n  uniq = {};\n  bus = {};\n  vendor = {};\n  product = {};\n  version = {};\n  country = {};\n  descriptor = {};\n}}\n",
            s(&self.name),
            s(&self.uniq),
            self.bus,
            self.vendor,
            self.product,
            self.version,
            self.country,
            s(&self.descriptor)
        )
    }
}
