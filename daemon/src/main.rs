//! `fanarchyd`: fan control daemon entry point.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use fanarchyd::config::{self, CONFIG_PATH};
use fanarchyd::control::{self, PanicHook};
use fanarchyd::gpu::{GpuSensor, NvidiaSmi};
use fanarchyd::hwmon::{Hwmon, SysHwmon};
use fanarchyd::status::render_status;
use tokio::signal::unix::{SignalKind, signal};
use tracing_subscriber::EnvFilter;

/// Fan control daemon for Omarchy.
#[derive(Debug, Parser)]
#[command(name = "fanarchyd", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Control the fans listed in the config until stopped.
    Run {
        #[arg(long, default_value = CONFIG_PATH)]
        config: PathBuf,
    },
    /// Validate a config file and check it against the detected hardware
    /// (read-only).
    CheckConfig { path: PathBuf },
    /// Set pwm_enable back to automatic for every `role: fan` output in
    /// the config. Used as the systemd ExecStopPost hook.
    Restore {
        #[arg(long, default_value = CONFIG_PATH)]
        config: PathBuf,
    },
    /// Print every detected sensor, fan and pwm output (read-only).
    Status,
}

fn main() -> Result<()> {
    init_logging();
    let cli = Cli::parse();
    match cli.command {
        Command::Run { config } => run(&config),
        Command::CheckConfig { path } => check_config(&path),
        Command::Restore { config } => restore(&config),
        Command::Status => status(),
    }
}

fn run(path: &Path) -> Result<()> {
    tracing::info!(version = env!("CARGO_PKG_VERSION"), config = %path.display(), "fanarchyd starting");
    let config = config::load(path)?;
    for warning in config::warnings(&config) {
        tracing::warn!("{warning}");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("cannot start the async runtime")?;
    runtime.block_on(async {
        let mut terminate = signal(SignalKind::terminate()).context("cannot listen for SIGTERM")?;
        let mut interrupt = signal(SignalKind::interrupt()).context("cannot listen for SIGINT")?;
        let shutdown = async move {
            tokio::select! {
                _ = terminate.recv() => tracing::info!("received SIGTERM"),
                _ = interrupt.recv() => tracing::info!("received SIGINT"),
            }
        };
        let hwmon = Arc::new(SysHwmon::default());
        control::run(hwmon, &config, PanicHook::Install, shutdown).await?;
        Ok(())
    })
}

fn check_config(path: &Path) -> Result<()> {
    let config = config::read(path)?;
    if let Err(errors) = config::validate(&config) {
        for error in &errors {
            println!("error: {error}");
        }
        bail!("{} is invalid ({} errors)", path.display(), errors.len());
    }
    for warning in config::warnings(&config) {
        println!("warning: {warning}");
    }

    let chips = match SysHwmon::default().chips() {
        Ok(chips) => chips,
        Err(err) => {
            println!("note: hardware not checked: {err}");
            println!("{} is valid", path.display());
            return Ok(());
        }
    };
    match config::resolve(&config, &chips) {
        Ok(plan) => {
            for fan in &plan.fans {
                println!(
                    "ok: fan '{}' -> {}/pwm{} following {}",
                    fan.id,
                    fan.output.chip_path.display(),
                    fan.output.index,
                    fan.sensor.name
                );
            }
            println!("{} is valid and matches the hardware", path.display());
            Ok(())
        }
        Err(errors) => {
            for error in &errors {
                println!("error: {error}");
            }
            bail!("{} does not match the detected hardware", path.display());
        }
    }
}

fn restore(path: &Path) -> Result<()> {
    // Best effort: a config that no longer validates is still used, since
    // this runs after a crash. Pumps are skipped either way.
    let config = config::read(path)?;
    if let Err(errors) = config::validate(&config) {
        for error in errors {
            tracing::warn!("config: {error}");
        }
    }
    let report = control::restore_from_config(&SysHwmon::default(), &config)?;
    for error in &report.errors {
        tracing::error!("{error}");
    }
    if !report.is_ok() {
        bail!(
            "restore incomplete: {} restored, {} failed, {} not found",
            report.restored.len(),
            report.failed.len(),
            report.errors.len()
        );
    }
    tracing::info!(
        fans = report.restored.len(),
        "every controlled fan is back to automatic"
    );
    Ok(())
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

/// Logs go to stderr so command output on stdout stays clean.
fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
