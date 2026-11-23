//! `fanarchyd`: fan control daemon entry point.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use fanarchy_protocol::table::render_status;
use fanarchyd::config::{self, CONFIG_PATH};
use fanarchyd::control::{self, PanicHook, Service};
use fanarchyd::gpu::{GpuSensor, NvidiaSmi};
use fanarchyd::hwmon::{Hwmon, SysHwmon};
use fanarchyd::ipc::{self, SOCKET_PATH};
use fanarchyd::status::GpuReading;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::{mpsc, watch};
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
        /// Where to create the IPC socket.
        #[arg(long, default_value = SOCKET_PATH)]
        socket: PathBuf,
    },
    /// Validate a config file and check it against the detected hardware
    /// (read-only).
    CheckConfig {
        #[arg(default_value = CONFIG_PATH)]
        path: PathBuf,
    },
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
        Command::Run { config, socket } => run(&config, &socket),
        Command::CheckConfig { path } => check_config(&path),
        Command::Restore { config } => restore(&config),
        Command::Status => status(),
    }
}

fn run(path: &Path, socket: &Path) -> Result<()> {
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
        let (listener, _socket_file) = ipc::bind(socket, ipc::lookup_socket_group())?;
        let (requests_tx, requests) = mpsc::channel(16);
        let (published_tx, published) = watch::channel(None);
        let (gpu_tx, gpu) = watch::channel(GpuReading::default());
        let context = ipc::Context {
            requests: requests_tx,
            published,
            gpu,
        };
        let server = tokio::spawn(ipc::serve(listener, context));
        let gpu_poller = tokio::spawn(ipc::poll_gpus(NvidiaSmi::default(), gpu_tx));

        let service = Service {
            config_path: path.to_path_buf(),
            requests,
            published: published_tx,
        };
        let hwmon = Arc::new(SysHwmon::default());
        let result =
            control::serve(hwmon, config, PanicHook::Install, shutdown, Some(service)).await;
        server.abort();
        gpu_poller.abort();
        result?;
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
    for warning in config::tachometer_warnings(&config, &chips) {
        println!("warning: {warning}");
    }
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn parse(args: &[&str]) -> Command {
        Cli::try_parse_from(args).unwrap().command
    }

    #[test]
    fn check_config_defaults_to_the_daemon_config() {
        let Command::CheckConfig { path } = parse(&["fanarchyd", "check-config"]) else {
            panic!("expected check-config");
        };
        assert_eq!(path, PathBuf::from(CONFIG_PATH));
    }

    #[test]
    fn check_config_takes_an_explicit_path() {
        let Command::CheckConfig { path } = parse(&["fanarchyd", "check-config", "/tmp/x.json"])
        else {
            panic!("expected check-config");
        };
        assert_eq!(path, PathBuf::from("/tmp/x.json"));
    }

    #[test]
    fn run_and_check_config_share_the_default() {
        let Command::Run { config, .. } = parse(&["fanarchyd", "run"]) else {
            panic!("expected run");
        };
        assert_eq!(config, PathBuf::from(CONFIG_PATH));
    }
}
