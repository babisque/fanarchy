//! `fanarchyd`: fan control daemon entry point.

use anyhow::Result;
use clap::{Parser, Subcommand};
use fanarchyd::gpu::{GpuSensor, NvidiaSmi};
use fanarchyd::hwmon::{Hwmon, SysHwmon};
use fanarchyd::status::render_status;
use tracing_subscriber::EnvFilter;

/// Fan control daemon for Omarchy.
#[derive(Debug, Parser)]
#[command(name = "fanarchyd", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Print every detected sensor, fan and pwm output (read-only).
    Status,
}

fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();
    match cli.command {
        Some(Command::Status) => status(),
        None => {
            tracing::info!(version = env!("CARGO_PKG_VERSION"), "fanarchyd starting");
            Ok(())
        }
    }
}

fn status() -> Result<()> {
    let chips = SysHwmon::default().chips()?;
    let (gpus, gpu_note) = match NvidiaSmi::default().read() {
        Ok(gpus) => (gpus, None),
        Err(err) => (Vec::new(), Some(err.to_string())),
    };
    print!("{}", render_status(&chips, &gpus, gpu_note.as_deref()));
    Ok(())
}

/// Logs go to stderr so `status` output on stdout stays clean.
fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
