# Spec — Phase 7 (optional): FanControl Importer

## 1. Overview and Goal
* **Purpose:** Let people coming from Windows bring over the curves they used in FanControl, converting its config into the Fanarchy format.
* **Success Criteria:**
  * `fanarchyctl import-fancontrol path/userConfig.json` reads the FanControl config and lists the curves it finds, with names and points.
  * The user maps each imported curve to a Fanarchy fan through an interactive terminal prompt.
  * The result is a new config, validated by the same rules as Phase 2, saved to a separate file for review.
  * It works with my real backup (`FanControl/Configurations`, inside `D-backup.zip`).

## 2. Tech Stack and Architecture
* **Core:** Rust, in the CLI crate.
* **Parsing:** `serde_json` with partial types. Read only what we need (point curves and linear curves) and ignore the rest.
* **Conversion:** pure functions (FanControl curve → Fanarchy curve), with table-driven tests.
* **Sensors:** Windows sensor names do not exist on Linux, so the user picks the sensor for each curve. No guessing.

## 3. Commands
* **Test:** `cargo test`
* **Import:** `cargo run --bin fanarchyctl -- import-fancontrol userConfig.json --out config.imported.json`

## 4. Project Structure
* `cli/src/import/fancontrol.rs`: partial types and conversion.
* `cli/tests/fixtures/fancontrol/`: example configs, including a copy of my backup.

## 5. Code Style and Conventions
* Unknown FanControl curve types produce a clear warning ("curve of type X skipped"), never an error.
* Converted points are clamped to Fanarchy's minimum and maximum, with a warning when that happens.

## 6. Boundaries
* **Always:**
  * Save the result to a new file. The user reviews it and applies it with `reload`.
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
* **Never:**
  * NEVER overwrite `/etc/fanarchy/config.json` directly from the importer.
  * NEVER map an imported curve to a `pump` output.
