# Spec — Phase 1: Sensor Reading

## 1. Overview and Goal
* **Purpose:** Build the foundation of the `fanarchyd` daemon: detect hardware and read temperatures, fan speeds and the state of `pwm` outputs. This phase is **read-only** and writes nothing to hardware.
* **Success Criteria:**
  * The daemon finds every chip under `/sys/class/hwmon` and identifies each one by `name` (e.g. `it8689`, `k10temp`), never by its `hwmonN` index.
  * It lists every temperature (`tempN_input`), fan (`fanN_input`) and controllable output (`pwmN` + `pwmN_enable`), with labels (`*_label`) when present.
  * It reads the NVIDIA GPU temperature through `nvidia-smi`. Without an NVIDIA GPU, it carries on without errors.
  * `fanarchyd status` (temporary, built into the binary) prints a table with all of the above, and the values match `sensors`.
  * All logic works against a fake `/sys` in tests.

## 2. Tech Stack and Architecture
* **Core:** stable Rust (edition 2024), Cargo workspace.
* **Errors:** `thiserror` in the library, `anyhow` in the binary.
* **Logging:** `tracing` + `tracing-subscriber`.
* **Abstraction:** a `Hwmon` trait with a real implementation (`SysHwmon`, taking a root path, default `/sys/class/hwmon`) and a fake one for tests (`FakeHwmon`, built in a temp directory with `tempfile`).
* **Units:** the kernel reports millidegrees Celsius. Convert at the edge and use `f32` °C everywhere else. PWM is `u8` (0–255) internally and shown as %.

## 3. Commands
* **Build:** `cargo build`
* **Test:** `cargo test`
* **Lint:** `cargo clippy -- -D warnings`
* **Run:** `cargo run --bin fanarchyd -- status` (reading does not need root)

## 4. Project Structure
* `Cargo.toml`: workspace.
* `daemon/src/hwmon.rs`: `Hwmon` trait, `SysHwmon`, and the `Chip`, `TempSensor`, `Fan` and `PwmOutput` types.
* `daemon/src/gpu.rs`: GPU reading via `nvidia-smi`, behind a trait so it is testable.
* `daemon/src/main.rs`: the `status` subcommand.
* `daemon/tests/fixtures/`: fake `/sys` trees, including one matching IT8689 + k10temp.

## 5. Code Style and Conventions
* Small, pure functions; file reads happen only in `SysHwmon`.
* A missing or unreadable file means the sensor is "unavailable", not a fatal error.
* No `unwrap()`/`expect()` outside tests. No `unsafe`.
* `status` output is aligned, with units (°C, RPM, %) and the chip name.

## 6. Boundaries
* **Always:**
  * Write `FakeHwmon` tests for every kind of file read.
  * Treat out-of-range values (e.g. −127 °C or 0 RPM) as "unavailable" or "stopped" without crashing.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
* **Never:**
  * NEVER write to any file under `/sys` in this phase.
  * NEVER require root to read sensors.
  * NEVER hardcode a chip name or an `hwmonN` index.
