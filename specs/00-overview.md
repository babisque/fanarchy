# Project Spec: Fanarchy

## 1. Overview and Goal
* **Purpose:** Fan control for Omarchy, inspired by FanControl on Windows. A Rust daemon applies fan curves through `hwmon`, and an Omarchy shell plugin (`babisque.fanarchy`) shows temperatures and fan speeds in the bar and lets the user edit curves in a panel.
* **Audience:** Omarchy users on any motherboard whose `pwm` outputs are exposed by the kernel. Hardware is never hardcoded: everything comes from auto-detection and configuration.
* **Success Criteria:**
  * The daemon auto-detects `hwmon` chips, temperature sensors and controllable `pwm` outputs.
  * The user names each fan, picks its temperature sensor and marks which outputs are **pumps** (never controlled).
  * Curves (temperature → PWM %) are applied continuously and react to temperature changes within 2 s.
  * The bar widget shows the CPU temperature and a fan indicator, refreshed every 2 s.
  * The panel lets the user edit a curve by dragging points, applied immediately.
  * If the daemon stops, crashes or loses a sensor, fans fall back to the motherboard's automatic control or to 100%. They never stop.
  * Installation works with `omarchy plugin add <repo>` plus an AUR package with the daemon and its systemd service.

## 2. Tech Stack and Architecture
* **Daemon (`fanarchyd`):** stable Rust (edition 2024), `tokio`, `serde` + `serde_json`, `anyhow` (binary) and `thiserror` (libraries), `tracing` for logs. Runs as a root systemd service.
* **CLI (`fanarchyctl`):** Rust + `clap`. Reads status, lists sensors and reloads config. Talks to the daemon over the same socket as the plugin.
* **Plugin:** QML + JavaScript on the Omarchy shell (Quickshell). Kinds `bar-widget` and `panel`.
* **IPC:** JSON Lines over a Unix socket at `/run/fanarchy.sock`. Messages: `status`, `get_config`, `set_curve`, `set_mode`, `reload`, `subscribe`.
* **Config:** JSON at `/etc/fanarchy/config.json`, validated before it is applied. An invalid config is rejected and the previous one stays active.
* **GPU temperature (NVIDIA):** `nvidia-smi --query-gpu=temperature.gpu` in v1. NVML later.
* **Flow:** sensors → curves (pure functions) → target PWM → safety limits → write to `hwmon` → status to the plugin.

## 3. Commands
* **Build:** `cargo build`
* **Test:** `cargo test`
* **Lint:** `cargo clippy -- -D warnings`
* **Format:** `cargo fmt`
* **Daemon (dev):** `cargo build && sudo ./target/debug/fanarchyd`
* **Validate plugin:** `omarchy plugin validate plugin/`
* **Enable plugin:** `omarchy plugin enable babisque.fanarchy --section right`
* **Plugin logs:** run `omarchy-shell` from a terminal

## 4. Project Structure
* `daemon/src/hwmon.rs`: `Hwmon` trait and the real implementation over `/sys/class/hwmon`. The only place that touches hardware.
* `daemon/src/curve.rs`: curve math (interpolation, hysteresis, smoothing). Pure functions only.
* `daemon/src/control.rs`: control loop and safety rules.
* `daemon/src/config.rs`: config loading, validation and schema.
* `daemon/src/ipc.rs`: Unix socket and messages.
* `cli/`: `fanarchyctl`.
* `plugin/`: `manifest.json`, `Bar.qml`, `Panel.qml` and `models/` (JS logic, separate from the UI).
* `packaging/`: `fanarchyd.service` and `PKGBUILD`.
* `specs/`: this overview and one spec per phase.

## 5. Code Style and UX Conventions
* **Omarchy theme:** use the active shell theme's colors and fonts. No hardcoded colors: the plugin must look right on every theme.
* **Compact widget:** icon + CPU temperature in the bar. Details live in the panel.
* **Curve editor:** draggable points, X axis in °C and Y axis in %, with a marker showing the current temperature moving along the curve.
* **Units always visible:** °C, RPM and %.
* **Daemon offline:** never show a blank screen. Show the state ("Daemon stopped") and the command to start it.
* **Loading:** never show "Loading...". Show the panel layout with placeholder values.
* **Feedback:** every applied change shows a subtle confirmation; every error shows a clear message.
* **Code:** small, pure functions, no `unwrap()`/`expect()` outside tests, no `unsafe`, tests for every new feature.

## 6. Boundaries
* **Always:**
  * Restore every controlled `pwmN_enable` to its original value (usually `2`, automatic) when the daemon stops or crashes.
  * Drive fans to 100% if a temperature sensor fails or the CPU goes above 85 °C.
  * Enforce a configurable minimum PWM and never drive a controlled fan to 0.
  * Validate the config before applying it.
  * Find chips by `name`, never by `hwmonN`, which changes between boots.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before running any command that writes to real hardware files under `/sys`. Propose the command and let me run it.
* **Never:**
  * NEVER control an output marked as a pump. On my machine, the pump is `fan2`/`pwm2`.
  * NEVER write to `/sys` outside the outputs listed in the user's config.
  * NEVER change system config: `/etc/modprobe.d/`, `/etc/modules-load.d/`, `/etc/fstab`, Limine or kernel parameters.
  * NEVER install, remove or rebuild kernel modules or DKMS packages.
  * NEVER rename socket messages, the config schema or the plugin ID.
  * NEVER build a generic, AI-template-looking UI. The widget must feel native to Omarchy.
