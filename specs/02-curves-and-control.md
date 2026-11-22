# Spec — Phase 2: Curves, Control and Safety

## 1. Overview and Goal
* **Purpose:** Make the daemon control fans: read the config, compute PWM from curves and write it to `hwmon`, always protected against failure. This is the most sensitive phase of the project.
* **Success Criteria:**
  * Curves (temperature → % points) are linearly interpolated, with hysteresis and smoothing on the way up and down.
  * The control loop runs every 1 s and reacts to a temperature change within 2 s.
  * Outputs marked as `pump`, or not in the config, are never touched.
  * On stop (SIGTERM or Ctrl+C), panic or error, every controlled `pwmN_enable` goes back to the original value read at startup.
  * If a sensor fails, the fans depending on it go to 100%.
  * If the CPU goes above the critical limit (default 85 °C), every controlled fan goes to 100%.
  * PWM never drops below the configured minimum and never reaches 0.
  * All of this logic is tested with `FakeHwmon`, without real hardware.

## 2. Tech Stack and Architecture
* **Core:** Rust + `tokio` (loop with `tokio::time::interval`, signals with `tokio::signal`).
* **Config:** `serde` + `serde_json`, at `/etc/fanarchy/config.json`, validated before it is applied.
* **Schema (v1):**
  * `chips`: identified by `name`.
  * `fans[]`: `id`, `label`, `chip`, `pwm` (number), `role` (`fan` or `pump`), `sensor`, `curve` (points), `min_percent`. Optional on both roles: `rpm_input`, the `N` of the `fanN_input` tachometer when it differs from `pwm`.
  * `safety`: `critical_temp_c`, `sensor_fail_percent`.
* **Flow:** read sensors → `curve::evaluate` (pure) → `safety::clamp` (pure) → `Hwmon::write_pwm`.
* **Restore:** a drop guard, a signal handler and a panic hook, all calling the same restore function.

## 3. Commands
* **Test:** `cargo test`
* **Lint:** `cargo clippy -- -D warnings`
* **Validate config:** `cargo run --bin fanarchyd -- check-config path/config.json`
* **Run for real:** `sudo ./target/debug/fanarchyd run` (**I run this, not Claude**)

## 4. Project Structure
* `daemon/src/curve.rs`: interpolation, hysteresis and smoothing. Pure functions only.
* `daemon/src/safety.rs`: minimum, critical limit and sensor failure. Pure functions only.
* `daemon/src/config.rs`: schema, loading and validation.
* `daemon/src/control.rs`: the loop and the restore logic.
* `examples/config.example.json`: an example for my board (fan1/fan3 controlled, fan2 as pump).

## 5. Code Style and Conventions
* Curve and safety math stays in pure functions, with table-driven tests (many input/output cases).
* Only write a PWM value when it changed, to avoid hitting the hardware every second for nothing.
* Every hardware write logs at `debug`; every safety action logs at `warn`.

## 6. Boundaries
* **Always:**
  * Save the original `pwmN_enable` of each output before the first write.
  * Write a test for each safety rule before implementing the loop.
  * Refuse to start if the config is invalid, a `pump` output has a curve, or two fans point to the same `pwm`.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before any test on real hardware. Propose the command and I run it.
* **Never:**
  * NEVER write to an output with `role: pump` or one missing from the config.
  * NEVER write PWM 0 or anything below the minimum.
  * NEVER run the daemon with `sudo` on your own.
  * NEVER change the config schema after it is approved without asking.
