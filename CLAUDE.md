# fanarchy

Fan control for Omarchy: a root daemon that applies fan curves through `hwmon`, plus an Omarchy shell plugin (`io.github.babisque.fanarchy`) that shows temps/RPM in the bar and edits curves in a panel. A Linux take on the Windows app FanControl.

## Stack
- Daemon: Rust (stable, edition 2024), `tokio` async runtime, run as a systemd unit (root)
- Serialization: `serde` + `serde_json`
- Plugin: QML + JavaScript on the Omarchy shell (Quickshell), kinds `bar-widget` and `panel`
- IPC: JSON over a Unix socket at `/run/fanarchy.sock`
- Config: JSON at `/etc/fanarchy/config.json`
- Tests: `cargo test`
- Hardware: Gigabyte B550M AORUS ELITE, ITE IT8689E via the community `it87` DKMS driver (frankcrawford/it87), Ryzen 5 5600 (`k10temp`), GTX 1660 Super

## Hardware map
| Header | Sensor | Control | Notes |
| --- | --- | --- | --- |
| fan1 | `fan1_input` | `pwm1` | Case/radiator fans |
| fan2 | `fan2_input` | none | **AIO pump. Read-only. Never write to `pwm2`.** |
| fan3 | `fan3_input` | `pwm3` | Case fans |

- The `hwmon` index changes between boots. Find the device by name, never hardcode `hwmonN`. The out-of-tree driver reports `it8689_61030507`; the suffix may change between driver versions, so the config uses `it8689` and matches `it8689` or `it8689_*` (more than one match refuses to start).
- `pwmN_enable`: `1` = manual, `2` = automatic (BIOS). Original value is `2`.

## Run commands
- build: `cargo build`
- test: `cargo test`
- lint: `cargo clippy -- -D warnings`
- format: `cargo fmt`
- run daemon (dev): `cargo build && sudo ./target/debug/fanarchyd`
- validate plugin: `omarchy plugin validate .` (manifest.json lives at the repo root)
- test plugin models: `node --test plugin/tests/`
- enable plugin: `omarchy plugin enable io.github.babisque.fanarchy --section right`
- plugin logs: run `omarchy-shell` from a terminal
- read sensors: `sensors`

## Conventions
- Prefer small, pure functions, especially for curve math (temp in, PWM out).
- No complex abstractions or clever generics. Clarity over brevity.
- Every new feature needs tests.
- Hardware access goes through one thin trait (`Hwmon`) so everything else is testable without real hardware.
- No `unwrap()` / `expect()` outside tests. Return errors with `anyhow` in the binary and `thiserror` in library code.
- No `unsafe`.
- In QML, keep logic in JS models; QML files only describe the UI.
- Code, comments and commit messages in English.

## Safety rules (always)
- Only write to `pwm1`, `pwm3` and their `_enable` files. Nothing else under `/sys`.
- On daemon stop, crash or unhandled exception: set every controlled `pwmN_enable` back to `2`.
- If a temperature sensor can't be read: set the affected fans to 100%.
- Enforce a minimum PWM floor from config. Never set a controlled fan to 0.
- CPU above 85 °C: all controlled fans to 100%, regardless of curve.

## Off-limits (never touch without explicit instruction)
- `pwm2` / the pump, in any way.
- System config: `/etc/modprobe.d/`, `/etc/modules-load.d/`, `/etc/fstab`, Limine/bootloader files, kernel parameters.
- Installing, removing or rebuilding kernel modules or DKMS packages.
- Running anything that writes to real `/sys` hardware files. Propose the command and let me run it.

## Fallbacks (when in doubt)
- Propose 2 options and ask me to pick.
- Never rename the IPC messages, the config schema or plugin IDs without asking.
- If unsure how a hardware file behaves, stop and ask instead of experimenting.
