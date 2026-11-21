//! `fanarchyctl`: talks to `fanarchyd` over its Unix socket.
//!
//! Subcommands (`status`, `config`, `set-curve`, `set-mode`, `reload`)
//! arrive in Phase 3.

use anyhow::Result;
use clap::Parser;

/// Control and inspect the fanarchy fan daemon.
#[derive(Debug, Parser)]
#[command(name = "fanarchyctl", version, about)]
struct Cli {}

fn main() -> Result<()> {
    let _cli = Cli::parse();
    Ok(())
}
