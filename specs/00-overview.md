# Project Spec: Fanarchy

## 1. Overview and Goal
* **Purpose:** Fan control for Omarchy, inspired by FanControl on Windows. A Rust daemon applies fan curves through `hwmon`, and an Omarchy shell plugin (`io.github.babisque.fanarchy`) shows temperatures and fan speeds in the bar and lets the user edit curves in a panel.
* **Audience:** Omarchy users on any motherboard whose `pwm` outputs are exposed by the kernel. Hardware is never hardcoded: everything comes from auto-detection and configuration.
* **Success Criteria:**
  * The daemon auto-detects `hwmon` chips, temperature sensors and controllable `pwm` outputs.
  * The user names each fan, picks its temperature sensor and marks which outputs are **pumps** (never controlled).
  * Curves (temperature → PWM %) are applied continuously and react to temperature changes within 2 s.
  * The bar widget shows the CPU temperature and a fan indicator, refreshed every 2 s.
  * The panel lets the user edit a curve by dragging points, applied immediately.
  * If the daemon stops, crashes or loses a sensor, fans fall back to the motherboard's automatic control or to 100%. They never stop.
  * The plugin is listed on the Omarchy marketplace (plugins.omarchy.org) and installs with `omarchy plugin add`. The daemon ships as an AUR package.

## 2. Tech Stack and Architecture
* **Daemon (`fanarchyd`):** stable Rust (edition 2024), `tokio`, `serde` + `serde_json`, `anyhow` (binary) and `thiserror` (libraries), `tracing` for logs. Runs as a root systemd service.
* **CLI (`fanarchyctl`):** Rust + `clap`. Reads status, lists sensors and reloads config. Talks to the daemon over the same socket as the plugin.
* **Plugin:** QML + JavaScript on the Omarchy shell (Quickshell). Kinds `bar-widget` and `panel`. `manifest.json` lives at the **repository root** (a marketplace requirement); the QML files stay under `plugin/` and are referenced by relative paths.
* **IPC:** JSON Lines over a Unix socket at `/run/fanarchy.sock`. Messages: `status`, `get_config`, `set_curve`, `set_mode`, `reload`, `subscribe`.
* **Config:** JSON at `/etc/fanarchy/config.json`, validated before it is applied. An invalid config is rejected and the previous one stays active.
* **GPU temperature (NVIDIA):** `nvidia-smi --query-gpu=temperature.gpu` in v1. NVML later.
* **Flow:** sensors → curves (pure functions) → target PWM → safety limits → write to `hwmon` → status to the plugin.

## 3. Commands
* **Build:** `cargo build`
* **Test:** `cargo test`
* **Lint:** `cargo clippy -- -D warnings`
* **Format:** `cargo fmt`
* **Daemon (dev):** `cargo build && sudo ./target/debug/fanarchyd run`
* **Validate plugin:** `omarchy plugin validate .`
* **Enable plugin:** `omarchy plugin enable io.github.babisque.fanarchy --section right`
* **Plugin logs:** run `omarchy-shell` from a terminal

## 4. Project Structure
* `manifest.json`: plugin manifest, at the repository root.
* `daemon/`: `fanarchyd` (hwmon, curves, safety, control loop, IPC server).
* `cli/`: `fanarchyctl`.
* `protocol/`: shared IPC message types.
* `plugin/`: `BarWidget.qml`, `Panel.qml`, `components/`, `models/` (JS logic, separate from the UI) and `assets/` (runtime images and fonts).
* `design/`: brand sources, never loaded at runtime (`logo.svg`, `logo-mark.svg`, `logo-reference.jpg`, `logo-animated.gif`).
* `docs/`: `protocol.md`, `hardware-notes.md` and `assets/` (README images).
* `packaging/`: `fanarchyd.service`, `PKGBUILD`, `fanarchy.sysusers`.
* `specs/`: this overview and one spec per phase.

## 5. Visual Identity
The look is a punk, DIY zine: photocopy texture, torn masking tape, stencil type and one signal red. Fanarchy keeps its own palette instead of following the Omarchy theme.

* **Mockups (source of truth for layout):** https://claude.ai/artifact/2DN7a7fuDpvGY8XJ6HBenA. The link needs my login; when a phase needs a screen, I paste the relevant screenshot or values. The numbers and rules below are what the code must follow.
* **Palette** (define once in `plugin/models/theme.js`, never inline elsewhere):

  | Token | Hex | Use |
  | --- | --- | --- |
  | `ink` | `#121211` | Backgrounds, text on paper |
  | `inkRaised` | `#1B1B1A` | Chart area, code blocks |
  | `line` | `#2E2D2A` | Borders, grid lines |
  | `paper` | `#ECE8DE` | Main text on ink, selected cards, tape labels |
  | `ash` | `#9A968C` | Secondary text, disabled state |
  | `riot` | `#E23B2E` | Accent: logo, warning, current-temperature marker, danger zone. Never for small text. |

* **Fonts** (bundled in `plugin/assets/fonts/` with their license files, never loaded from the network):
  * **Big Shoulders Stencil Display** 900 (SIL OFL): headings and the wordmark.
  * **JetBrains Mono** 400/700 (SIL OFL): all data (°C, RPM, %).
  * **Special Elite** (Apache 2.0): short notes and empty-state copy only.
* **Assets:**

  | File | Used by |
  | --- | --- |
  | `design/logo.svg` | Panel header, README. The full stenciled Ⓐ with the fan in the middle. |
  | `design/logo-mark.svg` | Bar icon at 16–24 px (ring + fan, no A). Copied to `plugin/assets/` at build time. |
  | `plugin/assets/paper-grain.png` | Panel background, tiled at 512 × 512 px. |
  | `plugin/assets/tape-1.png` … `tape-3.png` | Decorative tape on the tooltip, the pump card and empty states. |
  | `docs/assets/banner.jpg` | README hero. |
  | `docs/assets/social-preview.jpg` | GitHub social preview (1280 × 640). |
  | `design/logo-animated.gif` | README only. Never shipped with the plugin. |

## 6. Code Style and UX Conventions
* **Compact widget:** icon + CPU temperature in the bar. Details live in the panel.
* **Curve editor:** draggable points, X axis in °C and Y axis in %, with a marker showing the current temperature moving along the curve.
* **Units always visible:** °C, RPM and %.
* **Daemon offline:** never show a blank screen. Show the state ("Daemon stopped") and the command to start it.
* **Loading:** never show "Loading...". Show the panel layout with placeholder values.
* **Feedback:** every applied change shows a subtle confirmation; every error shows a clear message.
* **Contrast:** text meets 4.5:1. `riot` red is for shapes, marks and large text (18.66 px bold or more), never for body text.
* **Code:** small, pure functions, no `unwrap()`/`expect()` outside tests, no `unsafe`, tests for every new feature.

## 7. Boundaries
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
  * NEVER load fonts, images or anything else from the network in the plugin.
  * NEVER build a generic, AI-template-looking UI. Follow the mockups and the tokens above.
