//! `fanarchyctl`: talks to `fanarchyd` over its Unix socket.
//!
//! One request per run: connect, send one JSON line, print the reply.
//! The protocol is described in `docs/protocol.md`.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use fanarchy_protocol::table::{render_fans, render_status};
use fanarchy_protocol::{CurvePoint, FanMode, Request, Response, SOCKET_PATH, Status};
use serde_json::Value;

/// How long to wait for the daemon to answer.
const TIMEOUT: Duration = Duration::from_secs(5);

/// Control and inspect the fanarchy fan daemon.
#[derive(Debug, Parser)]
#[command(name = "fanarchyctl", version, about)]
struct Cli {
    /// Path of the daemon's socket.
    #[arg(long, global = true, default_value = SOCKET_PATH)]
    socket: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Show every sensor, then each configured fan.
    Status,
    /// Print the config the daemon is running with.
    Config,
    /// Replace a fan's curve (applied now and saved to the config).
    SetCurve {
        /// Fan id from the config, e.g. `case`.
        fan: String,
        /// Points as TEMP:PERCENT pairs, e.g. `30:25,60:50,80:100`.
        points: String,
    },
    /// Switch a fan's mode (not saved; resets to `curve` on restart).
    SetMode {
        /// Fan id from the config, e.g. `case`.
        fan: String,
        mode: ModeArg,
        /// Speed in % for `fixed`.
        #[arg(long)]
        percent: Option<u8>,
    },
    /// Re-read the config file.
    Reload,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ModeArg {
    /// Follow the fan's curve.
    Curve,
    /// A fixed speed (needs --percent).
    Fixed,
    /// Motherboard control (the daemon still forces 100 % when critical).
    Auto,
}

impl From<ModeArg> for FanMode {
    fn from(mode: ModeArg) -> Self {
        match mode {
            ModeArg::Curve => FanMode::Curve,
            ModeArg::Fixed => FanMode::Fixed,
            ModeArg::Auto => FanMode::Auto,
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> Result<()> {
    match &cli.command {
        Command::Status => {
            let data = request(&cli.socket, &Request::Status)?;
            let status: Status =
                serde_json::from_value(data).context("unexpected status from the daemon")?;
            print!(
                "{}",
                render_status(&status.chips, &status.gpus, status.gpu_note.as_deref())
            );
            println!();
            print!("{}", render_fans(&status.fans));
        }
        Command::Config => {
            let data = request(&cli.socket, &Request::GetConfig)?;
            println!("{}", serde_json::to_string_pretty(&data)?);
        }
        Command::SetCurve { fan, points } => {
            let points = parse_points(points)?;
            let request_body = Request::SetCurve {
                fan: fan.clone(),
                points,
            };
            request(&cli.socket, &request_body)?;
            println!("fan '{fan}': curve updated and saved");
        }
        Command::SetMode { fan, mode, percent } => {
            let mode = FanMode::from(*mode);
            let request_body = Request::SetMode {
                fan: fan.clone(),
                mode,
                percent: *percent,
            };
            request(&cli.socket, &request_body)?;
            match percent {
                Some(percent) => println!("fan '{fan}': mode {} at {percent} %", mode.as_str()),
                None => println!("fan '{fan}': mode {}", mode.as_str()),
            }
        }
        Command::Reload => {
            request(&cli.socket, &Request::Reload)?;
            println!("config reloaded");
        }
    }
    Ok(())
}

/// Send one request and return the reply's `data` (`null` when empty).
fn request(socket: &Path, request: &Request) -> Result<Value> {
    let stream = UnixStream::connect(socket)
        .map_err(|err| anyhow::anyhow!(connect_error(socket, err.kind(), &err.to_string())))?;
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;

    let mut line = serde_json::to_string(request)?;
    line.push('\n');
    (&stream)
        .write_all(line.as_bytes())
        .context("cannot send the request")?;

    let mut reply = String::new();
    BufReader::new(&stream)
        .read_line(&mut reply)
        .context("no reply from fanarchyd")?;
    if reply.is_empty() {
        bail!("fanarchyd closed the connection without replying");
    }
    let response: Response =
        serde_json::from_str(&reply).context("unexpected reply from fanarchyd")?;
    if !response.ok {
        bail!(
            "{}",
            response
                .error
                .unwrap_or_else(|| "request failed".to_string())
        );
    }
    Ok(response.data.unwrap_or(Value::Null))
}

/// A connection error with what to do about it.
fn connect_error(socket: &Path, kind: ErrorKind, detail: &str) -> String {
    let path = socket.display();
    match kind {
        ErrorKind::NotFound | ErrorKind::ConnectionRefused => format!(
            "cannot connect to {path}: is fanarchyd running? (sudo systemctl start fanarchyd)"
        ),
        ErrorKind::PermissionDenied => format!(
            "cannot connect to {path}: permission denied; add yourself to the 'fanarchy' group (sudo usermod -aG fanarchy $USER, then log in again)"
        ),
        _ => format!("cannot connect to {path}: {detail}"),
    }
}

/// Parse `30:25,60:50,80:100` into curve points. The daemon validates
/// the curve itself (ranges, order).
fn parse_points(text: &str) -> Result<Vec<CurvePoint>> {
    text.split(',')
        .map(|pair| {
            let pair = pair.trim();
            let parsed = pair.split_once(':').and_then(|(temp, percent)| {
                Some((temp.trim().parse().ok()?, percent.trim().parse().ok()?))
            });
            match parsed {
                Some((temp, percent)) if f32::is_finite(temp) && f32::is_finite(percent) => {
                    Ok((temp, percent))
                }
                _ => bail!("invalid point '{pair}' (expected TEMP:PERCENT, e.g. 60:50)"),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_points() {
        assert_eq!(
            parse_points("30:25,60:50,80:100").unwrap(),
            vec![(30.0, 25.0), (60.0, 50.0), (80.0, 100.0)]
        );
        assert_eq!(
            parse_points(" 42.5 : 30 , 70:65.5").unwrap(),
            vec![(42.5, 30.0), (70.0, 65.5)]
        );
    }

    #[test]
    fn rejects_bad_points() {
        for (input, bad) in [
            ("30-25", "30-25"),
            ("30:25,", ""),
            ("30:abc", "30:abc"),
            ("nan:10", "nan:10"),
            ("30:25:1", "30:25:1"),
        ] {
            let err = parse_points(input).unwrap_err().to_string();
            assert_eq!(
                err,
                format!("invalid point '{bad}' (expected TEMP:PERCENT, e.g. 60:50)"),
                "{input}"
            );
        }
    }

    #[test]
    fn connect_errors_say_what_to_do() {
        let socket = Path::new(SOCKET_PATH);
        assert!(
            connect_error(socket, ErrorKind::NotFound, "")
                .ends_with("is fanarchyd running? (sudo systemctl start fanarchyd)")
        );
        assert!(
            connect_error(socket, ErrorKind::ConnectionRefused, "")
                .contains("is fanarchyd running?")
        );
        assert!(
            connect_error(socket, ErrorKind::PermissionDenied, "")
                .contains("add yourself to the 'fanarchy' group")
        );
        assert_eq!(
            connect_error(socket, ErrorKind::Other, "boom"),
            "cannot connect to /run/fanarchy.sock: boom"
        );
    }

    #[test]
    fn cli_parses_subcommands() {
        let cli = Cli::try_parse_from([
            "fanarchyctl",
            "set-mode",
            "case",
            "fixed",
            "--percent",
            "60",
        ])
        .unwrap();
        assert!(matches!(
            cli.command,
            Command::SetMode {
                mode: ModeArg::Fixed,
                percent: Some(60),
                ..
            }
        ));
        assert_eq!(cli.socket, PathBuf::from(SOCKET_PATH));

        let cli =
            Cli::try_parse_from(["fanarchyctl", "--socket", "/tmp/x.sock", "status"]).unwrap();
        assert_eq!(cli.socket, PathBuf::from("/tmp/x.sock"));
    }
}
