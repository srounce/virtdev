mod cache;
mod config;
mod daemon;
mod hidraw;
mod proxy;
mod uhid;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Print identity and report descriptor of a hidraw device.
    Inspect { path: PathBuf },
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
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match Cli::parse().cmd {
        Cmd::Inspect { path } => inspect(&path),
        Cmd::Mirror { path, name } => mirror(&path, name),
        Cmd::Daemon { config, cache_dir } => daemon::run(config::load(&config)?, &cache_dir),
    }
}

fn inspect(path: &std::path::Path) -> Result<()> {
    let dev = hidraw::Hidraw::open(path)?;
    let info = dev.info()?;
    println!("name:    {}", info.name);
    println!("phys:    {}", info.phys);
    println!("uniq:    {}", info.uniq);
    println!("id:      {:04x}:{:04x}:{:04x}", info.bus, info.vendor, info.product);
    println!("rdesc:   {} bytes", info.descriptor.len());
    for chunk in info.descriptor.chunks(16) {
        println!("  {}", proxy::hex(chunk));
    }
    Ok(())
}

fn mirror(path: &std::path::Path, name: Option<String>) -> Result<()> {
    let dev = hidraw::Hidraw::open(path)?;
    let info = dev.info()?;
    let ident = uhid::Identity {
        name: name.unwrap_or_else(|| info.name.clone()),
        phys: daemon::phys_for("mirror"),
        uniq: info.uniq.clone(),
        bus: info.bus,
        vendor: info.vendor as u32,
        product: info.product as u32,
        version: 0,
        country: 0,
        descriptor: info.descriptor.clone(),
    };
    log::info!("creating uhid clone of {} ({:04x}:{:04x}:{:04x})", info.name, info.bus, info.vendor, info.product);
    let virt = uhid::Uhid::create(&ident)?;
    let (proxy, handle) = proxy::Proxy::new("mirror".into(), virt)?;
    handle.send(proxy::Ctrl::Attach(dev));
    proxy.run()
}
