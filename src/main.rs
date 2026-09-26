mod cache;
mod config;
mod daemon;
mod hidraw;
mod identity;
mod proxy;
mod rules;
mod uhid;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    Text,
    Toml,
    Nix,
    Json,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print identity and report descriptor of a hidraw device.
    Inspect {
        path: PathBuf,
        /// Structured formats emit the identity as used by the config and NixOS module.
        #[arg(short, long, value_enum, default_value_t = Format::Text)]
        format: Format,
    },
    /// Create a uhid clone of a hidraw device and proxy reports until interrupted.
    Mirror {
        path: PathBuf,
        /// Name for the virtual device (defaults to the source name).
        #[arg(long)]
        name: Option<String>,
    },
    /// Run the proxy daemon for every device in the config file.
    Daemon {
        config: PathBuf,
        /// Where cached device identities live. Defaults to $CACHE_DIRECTORY.
        #[arg(long, env = "CACHE_DIRECTORY", default_value = "/var/cache/virtdev")]
        cache_dir: PathBuf,
    },
    /// Write 70-virtdev.rules and 99-virtdev.rules for the config into a directory.
    UdevRules {
        config: PathBuf,
        out_dir: PathBuf,
        /// User the daemon runs as.
        #[arg(long, default_value = "virtdev")]
        user: String,
        #[arg(long, default_value = "virtdev")]
        group: String,
        /// Absolute path of setfacl, used to strip ACLs from source nodes.
        #[arg(long, default_value = "/usr/bin/setfacl")]
        setfacl: String,
    },
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().cmd {
        Cmd::Inspect { path, format } => inspect(&path, format),
        Cmd::Mirror { path, name } => mirror(&path, name),
        Cmd::Daemon { config, cache_dir } => daemon::run(config::load(&config)?, &cache_dir),
        Cmd::UdevRules { config, out_dir, user, group, setfacl } => {
            let cfg = config::load(&config)?;
            let o = rules::Options { user: &user, group: &group, setfacl: &setfacl };
            std::fs::create_dir_all(&out_dir).with_context(|| format!("create {}", out_dir.display()))?;
            std::fs::write(out_dir.join("70-virtdev.rules"), rules::access(&o))?;
            std::fs::write(out_dir.join("99-virtdev.rules"), rules::hide(&cfg, &o))?;
            Ok(())
        }
    }
}

fn identity_of(path: &Path, phys: &str) -> Result<(hidraw::Hidraw, uhid::Identity)> {
    let dev = hidraw::Hidraw::open(path)?;
    let info = dev.info()?;
    let version = daemon::source_info_for(path).map(|s| s.version).unwrap_or(0);
    let ident = uhid::Identity {
        name: info.name,
        phys: phys.to_string(),
        uniq: info.uniq,
        bus: info.bus,
        vendor: info.vendor as u32,
        product: info.product as u32,
        version,
        country: 0,
        descriptor: info.descriptor,
    };
    Ok((dev, ident))
}

fn inspect(path: &Path, format: Format) -> Result<()> {
    let (dev, mut id) = identity_of(path, "")?;
    id.phys = dev.info()?.phys;
    let stored = identity::StoredIdentity::from_identity(&id);
    match format {
        Format::Toml => {
            print!("{}", toml::to_string(&stored)?);
            return Ok(());
        }
        Format::Json => {
            println!("{}", serde_json::to_string_pretty(&stored)?);
            return Ok(());
        }
        Format::Nix => {
            print!("{}", stored.to_nix());
            return Ok(());
        }
        Format::Text => {}
    }
    println!("name:    {}", id.name);
    println!("phys:    {}", id.phys);
    println!("uniq:    {}", id.uniq);
    println!("id:      {:04x}:{:04x}:{:04x} version {:04x}", id.bus, id.vendor, id.product, id.version);
    println!("rdesc:   {} bytes", id.descriptor.len());
    for chunk in id.descriptor.chunks(16) {
        println!("  {}", proxy::hex(chunk));
    }
    Ok(())
}

fn mirror(path: &Path, name: Option<String>) -> Result<()> {
    let (dev, mut ident) = identity_of(path, &daemon::phys_for("mirror"))?;
    if let Some(n) = name {
        ident.name = n;
    }
    log::info!("creating uhid clone of {} ({:04x}:{:04x}:{:04x})", ident.name, ident.bus, ident.vendor, ident.product);
    let virt = uhid::Uhid::create(&ident)?;
    let (proxy, handle) = proxy::Proxy::new("mirror".into(), virt)?;
    handle.send(proxy::Ctrl::Attach(dev));
    proxy.run()
}
