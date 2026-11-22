//! `fanarchyctl init`: detect the hardware and write a starter config.
//!
//! The starter config names the chips and the CPU sensor and controls
//! **nothing**: `fans` is empty. The user marks the pump and adds fans.
//! Only the hwmon tree is read; nothing under `/sys` is written.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

use anyhow::{Context, Result, bail};
use fanarchyd::config::{
    self, CONFIG_VERSION, Config, DEFAULT_CRITICAL_TEMP_C, DEFAULT_SENSOR_FAIL_PERCENT,
    SafetyConfig,
};
use fanarchyd::hwmon::{Chip, Hwmon, PwmMode, SysHwmon};

/// CPU temperature drivers, in order of preference.
pub const CPU_CHIPS: [&str; 3] = ["k10temp", "coretemp", "zenpower"];

/// Config alias of the CPU chip.
pub const CPU_ALIAS: &str = "cpu";

/// Why no starter config can be made.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum InitError {
    #[error(
        "no CPU temperature sensor found (looked for {}); the daemon needs one for its safety check",
        CPU_CHIPS.join(", ")
    )]
    NoCpuSensor,
    #[error("found {count} '{name}' chips; write the config by hand and pick one sensor")]
    AmbiguousCpu { name: String, count: usize },
    #[error("chip '{name}' has no temperature inputs")]
    CpuWithoutTemps { name: String },
}

/// The name to put in the config: the detected name without the
/// out-of-tree driver's hex suffix (`it8689_61030507` → `it8689`).
/// [`config::chip_name_matches`] still finds the chip with it.
pub fn config_chip_name(detected: &str) -> &str {
    match detected.rsplit_once('_') {
        Some((base, suffix))
            if !base.is_empty()
                && !suffix.is_empty()
                && suffix.chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            base
        }
        _ => detected,
    }
}

/// Alias for the `n`th chip with PWM outputs (0-based): `board`, `board2`…
pub fn board_alias(n: usize) -> String {
    if n == 0 {
        "board".to_string()
    } else {
        format!("board{}", n + 1)
    }
}

/// A config with the detected chips, the CPU sensor and no outputs.
pub fn starter_config(chips: &[Chip]) -> Result<Config, InitError> {
    let cpu = find_cpu(chips)?;
    let cpu_temp =
        cpu.temps
            .iter()
            .map(|t| t.index)
            .min()
            .ok_or_else(|| InitError::CpuWithoutTemps {
                name: cpu.name.clone(),
            })?;

    let mut aliases = BTreeMap::new();
    aliases.insert(
        CPU_ALIAS.to_string(),
        config_chip_name(&cpu.name).to_string(),
    );
    let mut boards: Vec<&str> = Vec::new();
    for chip in pwm_chips(chips) {
        let name = config_chip_name(&chip.name);
        if name != config_chip_name(&cpu.name) && !boards.contains(&name) {
            boards.push(name);
        }
    }
    for (n, name) in boards.iter().enumerate() {
        aliases.insert(board_alias(n), (*name).to_string());
    }

    Ok(Config {
        version: CONFIG_VERSION,
        chips: aliases,
        fans: Vec::new(),
        safety: SafetyConfig {
            critical_temp_c: DEFAULT_CRITICAL_TEMP_C,
            sensor_fail_percent: DEFAULT_SENSOR_FAIL_PERCENT,
            cpu_sensor: format!("{CPU_ALIAS}/temp{cpu_temp}"),
        },
    })
}

fn find_cpu(chips: &[Chip]) -> Result<&Chip, InitError> {
    for driver in CPU_CHIPS {
        let found: Vec<&Chip> = chips
            .iter()
            .filter(|chip| config::chip_name_matches(driver, &chip.name))
            .collect();
        match found.as_slice() {
            [] => continue,
            [chip] => return Ok(chip),
            many => {
                return Err(InitError::AmbiguousCpu {
                    name: driver.to_string(),
                    count: many.len(),
                });
            }
        }
    }
    Err(InitError::NoCpuSensor)
}

fn pwm_chips(chips: &[Chip]) -> impl Iterator<Item = &Chip> {
    chips.iter().filter(|chip| !chip.pwms.is_empty())
}

/// What `init` tells the user after writing the config: every PWM output
/// it saw, then what to do next.
pub fn summary(config: &Config, chips: &[Chip], written_to: Option<&Path>) -> String {
    let mut text = String::new();
    let pwm_chips: Vec<&Chip> = pwm_chips(chips).collect();
    if pwm_chips.is_empty() {
        text.push_str(
            "No PWM outputs found. On Gigabyte boards with an ITE chip, the in-kernel it87\n\
             driver may not support it: see \"Known issues\" in the README.\n",
        );
    } else {
        text.push_str("PWM outputs found (nothing is controlled yet):\n");
        for chip in pwm_chips {
            let alias = config
                .chips
                .iter()
                .find(|(_, name)| config::chip_name_matches(name, &chip.name))
                .map_or("?", |(alias, _)| alias.as_str());
            for pwm in &chip.pwms {
                let rpm = chip
                    .fans
                    .iter()
                    .find(|f| f.index == pwm.index)
                    .and_then(|f| f.rpm)
                    .map_or("no tach".to_string(), |rpm| {
                        format!("fan{} {rpm} RPM", pwm.index)
                    });
                let _ = writeln!(
                    text,
                    "  {alias} ({}) pwm{}: {rpm}, {}",
                    chip.name,
                    pwm.index,
                    mode_text(pwm.mode)
                );
            }
        }
    }
    let _ = writeln!(text, "CPU sensor: {}", config.safety.cpu_sensor);
    if let Some(path) = written_to {
        let _ = writeln!(text, "\nWrote {}.", path.display());
    }
    let path = written_to.map_or(config::CONFIG_PATH.into(), |p| p.display().to_string());
    text.push_str(
        "\nNext steps:\n\
         \x20 1. Mark the pump first: add it with \"role\": \"pump\" so it is never touched.\n\
         \x20 2. Add each fan you want controlled with \"role\": \"fan\", a sensor, a curve and min_percent.\n",
    );
    let _ = writeln!(text, "  3. Check it: fanarchyd check-config {path}");
    text.push_str(
        "  4. Start it: sudo systemctl enable --now fanarchyd\n\
         See examples/config.example.json and the README.\n",
    );
    text
}

fn mode_text(mode: Option<PwmMode>) -> &'static str {
    match mode {
        Some(PwmMode::Disabled) => "full speed",
        Some(PwmMode::Manual) => "manual",
        Some(PwmMode::Automatic(_)) => "automatic (BIOS)",
        None => "mode unknown",
    }
}

/// Detect the hardware and write (or print) the starter config.
pub fn run(output: &Path, force: bool, stdout: bool) -> Result<()> {
    let chips = SysHwmon::default()
        .chips()
        .context("cannot read the hwmon sensors")?;
    let config = starter_config(&chips)?;
    if let Err(errors) = config::validate(&config) {
        let list: Vec<String> = errors.iter().map(ToString::to_string).collect();
        bail!("the detected config is invalid: {}", list.join("; "));
    }

    if stdout {
        println!("{}", serde_json::to_string_pretty(&config)?);
        eprint!("{}", summary(&config, &chips, None));
        return Ok(());
    }

    write_config(output, &config, force)?;
    print!("{}", summary(&config, &chips, Some(output)));
    Ok(())
}

/// Save `config` at `output`, creating its directory. Refuses to replace
/// an existing file unless `force`.
pub fn write_config(output: &Path, config: &Config, force: bool) -> Result<()> {
    if output.exists() && !force {
        bail!(
            "{} already exists; pass --force to replace it, or --stdout to print the starter config",
            output.display()
        );
    }
    if let Some(dir) = output.parent().filter(|d| !d.as_os_str().is_empty())
        && !dir.exists()
    {
        DirBuilder::new()
            .recursive(true)
            .mode(0o755)
            .create(dir)
            .with_context(|| format!("cannot create {} (run with sudo?)", dir.display()))?;
    }
    config::save_atomic(output, config)
        .with_context(|| format!("cannot write {} (run with sudo?)", output.display()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::fs;
    use std::path::PathBuf;

    use fanarchyd::hwmon::{Fan, PwmOutput, TempSensor};

    use super::*;

    fn chip(name: &str, temps: &[u32], pwms: &[u32]) -> Chip {
        Chip {
            name: name.to_string(),
            path: PathBuf::from("/fake"),
            temps: temps
                .iter()
                .map(|&index| TempSensor {
                    index,
                    label: None,
                    celsius: Some(40.0),
                })
                .collect(),
            fans: pwms
                .iter()
                .map(|&index| Fan {
                    index,
                    label: None,
                    rpm: Some(1000),
                })
                .collect(),
            pwms: pwms
                .iter()
                .map(|&index| PwmOutput {
                    index,
                    value: Some(128),
                    mode: Some(PwmMode::Automatic(2)),
                })
                .collect(),
        }
    }

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../daemon/tests/fixtures/it8689_k10temp")
    }

    #[test]
    fn strips_only_hex_driver_suffixes() {
        assert_eq!(config_chip_name("it8689_61030507"), "it8689");
        assert_eq!(config_chip_name("k10temp"), "k10temp");
        assert_eq!(config_chip_name("asus_wmi_sensors"), "asus_wmi_sensors");
        assert_eq!(config_chip_name("nct6798"), "nct6798");
        assert_eq!(config_chip_name("_abc"), "_abc");
        assert_eq!(config_chip_name("it87_"), "it87_");
    }

    #[test]
    fn board_aliases_count_from_two() {
        assert_eq!(board_alias(0), "board");
        assert_eq!(board_alias(1), "board2");
        assert_eq!(board_alias(2), "board3");
    }

    #[test]
    fn starter_config_controls_nothing() {
        let chips = [
            chip("acpitz", &[1], &[]),
            chip("k10temp", &[1, 3], &[]),
            chip("it8689_61030507", &[1, 2], &[1, 2, 3]),
        ];
        let config = starter_config(&chips).unwrap();
        assert!(config.fans.is_empty());
        assert_eq!(config.chips.get("cpu").map(String::as_str), Some("k10temp"));
        assert_eq!(
            config.chips.get("board").map(String::as_str),
            Some("it8689")
        );
        assert_eq!(config.chips.len(), 2);
        assert_eq!(config.safety.cpu_sensor, "cpu/temp1");
        assert_eq!(config.safety.critical_temp_c, DEFAULT_CRITICAL_TEMP_C);
        assert_eq!(config::validate(&config), Ok(()));
    }

    #[test]
    fn prefers_k10temp_then_coretemp() {
        let chips = [chip("coretemp", &[1], &[]), chip("k10temp", &[1], &[])];
        let config = starter_config(&chips).unwrap();
        assert_eq!(config.chips["cpu"], "k10temp");

        let chips = [chip("coretemp", &[2, 3], &[])];
        let config = starter_config(&chips).unwrap();
        assert_eq!(config.chips["cpu"], "coretemp");
        assert_eq!(config.safety.cpu_sensor, "cpu/temp2");
    }

    #[test]
    fn cpu_problems_are_errors() {
        assert_eq!(
            starter_config(&[chip("it8689", &[1], &[1])]),
            Err(InitError::NoCpuSensor)
        );
        assert_eq!(
            starter_config(&[chip("coretemp", &[1], &[]), chip("coretemp", &[1], &[])]),
            Err(InitError::AmbiguousCpu {
                name: "coretemp".into(),
                count: 2
            })
        );
        assert_eq!(
            starter_config(&[chip("k10temp", &[], &[])]),
            Err(InitError::CpuWithoutTemps {
                name: "k10temp".into()
            })
        );
    }

    #[test]
    fn several_pwm_chips_get_numbered_aliases() {
        let chips = [
            chip("k10temp", &[1], &[]),
            chip("nct6798", &[1], &[1, 2]),
            chip("amdgpu", &[1], &[1]),
            chip("nct6798", &[1], &[1]),
        ];
        let config = starter_config(&chips).unwrap();
        assert_eq!(config.chips["board"], "nct6798");
        assert_eq!(config.chips["board2"], "amdgpu");
        assert_eq!(config.chips.len(), 3);
    }

    #[test]
    fn no_pwm_chips_still_gives_a_monitor_only_config() {
        let chips = [chip("k10temp", &[1], &[])];
        let config = starter_config(&chips).unwrap();
        assert_eq!(config.chips.len(), 1);
        assert!(summary(&config, &chips, None).contains("No PWM outputs found"));
    }

    #[test]
    fn summary_lists_outputs_and_next_steps() {
        let chips = [
            chip("k10temp", &[1], &[]),
            chip("it8689_61030507", &[1], &[1, 2]),
        ];
        let config = starter_config(&chips).unwrap();
        let text = summary(&config, &chips, Some(Path::new("/tmp/x.json")));
        assert!(text.contains("board (it8689_61030507) pwm1: fan1 1000 RPM, automatic (BIOS)"));
        assert!(text.contains("board (it8689_61030507) pwm2"));
        assert!(text.contains("Wrote /tmp/x.json."));
        assert!(text.contains("Mark the pump first"));
        assert!(text.contains("fanarchyd check-config /tmp/x.json"));
    }

    #[test]
    fn fixture_hardware_gives_a_config_that_resolves() {
        let chips = SysHwmon::new(fixture()).chips().unwrap();
        let config = starter_config(&chips).unwrap();
        assert_eq!(config.chips["cpu"], "k10temp");
        assert_eq!(config.chips["board"], "it8689");
        assert!(config.fans.is_empty());

        let plan = config::resolve(&config, &chips).unwrap();
        assert!(plan.fans.is_empty());
    }

    #[test]
    fn write_config_creates_the_directory_and_never_overwrites_silently() {
        let config =
            starter_config(&[chip("k10temp", &[1], &[]), chip("it8689", &[1], &[1])]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fanarchy/config.json");

        write_config(&path, &config, false).unwrap();
        assert_eq!(config::load(&path).unwrap(), config);
        assert!(!fs::read_to_string(&path).unwrap().contains("pwm"));

        fs::write(&path, "mine").unwrap();
        let err = write_config(&path, &config, false).unwrap_err();
        assert!(err.to_string().contains("--force"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), "mine");

        write_config(&path, &config, true).unwrap();
        assert_eq!(config::load(&path).unwrap(), config);
    }
}
