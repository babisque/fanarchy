//! `fanarchyd`: fan control daemon entry point.

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    init_logging();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "fanarchyd starting");
    Ok(())
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}
