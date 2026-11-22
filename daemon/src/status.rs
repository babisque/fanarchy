//! Build the IPC `status` payload from the daemon's state.
//!
//! Pure functions only: config + last control snapshot + GPU reading in,
//! [`Status`] out.

use fanarchy_protocol::{FanStatus, GpuTemp, Role, Status};

use crate::config::{self, Config, FanConfig};
use crate::control::ControlSnapshot;
use crate::hwmon::Chip;

/// The latest GPU reading, refreshed by the IPC server.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GpuReading {
    pub gpus: Vec<GpuTemp>,
    /// Why GPUs could not be read.
    pub note: Option<String>,
}

/// One entry per output in the config, pumps included.
pub fn build_status(config: &Config, snapshot: &ControlSnapshot, gpu: &GpuReading) -> Status {
    let fans = config
        .fans
        .iter()
        .map(|fan| fan_status(config, snapshot, fan))
        .collect();
    Status {
        fans,
        cpu_temp_c: snapshot.cpu_temp_c,
        critical_temp_c: config.safety.critical_temp_c,
        gpus: gpu.gpus.clone(),
        gpu_note: gpu.note.clone(),
        chips: snapshot.chips.clone(),
    }
}

fn fan_status(config: &Config, snapshot: &ControlSnapshot, fan: &FanConfig) -> FanStatus {
    let rpm = rpm_of(config, &snapshot.chips, fan);
    let mut status = FanStatus {
        id: fan.id.clone(),
        label: fan.label.clone(),
        role: fan.role,
        sensor: None,
        temp_c: None,
        rpm,
        percent: None,
        min_percent: None,
        mode: None,
        safety_override: None,
    };
    if fan.role == Role::Pump {
        return status;
    }
    status.sensor = fan.sensor.clone();
    status.min_percent = fan.min_percent;
    if let Some(controlled) = snapshot.fans.iter().find(|c| c.id == fan.id) {
        status.temp_c = controlled.temp_c;
        status.percent = controlled.percent.map(round_percent);
        status.mode = Some(controlled.mode.protocol());
        status.safety_override = Some(controlled.over);
    }
    status
}

/// `fanN_input` on the output's chip, with `N` from
/// [`config::rpm_input_of`].
fn rpm_of(config: &Config, chips: &[Chip], fan: &FanConfig) -> Option<u32> {
    let name = config.chips.get(&fan.chip)?;
    let chip = config::find_chip(chips, &fan.chip, name).ok()?;
    let input = config::rpm_input_of(fan);
    chip.fans.iter().find(|f| f.index == input)?.rpm
}

fn round_percent(percent: f32) -> u8 {
    // Clamped to 0–100 first, so the cast cannot truncate.
    percent.clamp(0.0, 100.0).round() as u8
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fanarchy_protocol::{FanMode, Override};

    use super::*;
    use crate::control::{FanSnapshot, Mode};
    use crate::hwmon::Fan;

    fn config() -> Config {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../examples/config.example.json");
        config::load(&path).unwrap()
    }

    fn board() -> Chip {
        Chip {
            name: "it8689_61030507".into(),
            path: PathBuf::from("/fake/hwmon5"),
            temps: Vec::new(),
            fans: (1..=3)
                .map(|index| Fan {
                    index,
                    label: None,
                    rpm: Some(1000 * index),
                })
                .collect(),
            pwms: Vec::new(),
        }
    }

    #[test]
    fn builds_one_entry_per_output() {
        let snapshot = ControlSnapshot {
            cpu_temp_c: Some(48.25),
            fans: vec![
                FanSnapshot {
                    id: "radiator".into(),
                    mode: Mode::Curve,
                    over: Override::None,
                    percent: Some(38.4),
                    temp_c: Some(48.25),
                },
                FanSnapshot {
                    id: "case".into(),
                    mode: Mode::Auto,
                    over: Override::None,
                    percent: None,
                    temp_c: Some(48.25),
                },
            ],
            chips: vec![board()],
        };
        let gpu = GpuReading {
            gpus: Vec::new(),
            note: Some("nvidia-smi failed".into()),
        };
        let status = build_status(&config(), &snapshot, &gpu);

        assert_eq!(status.cpu_temp_c, Some(48.25));
        assert_eq!(status.critical_temp_c, 85.0);
        assert_eq!(status.gpu_note.as_deref(), Some("nvidia-smi failed"));
        let ids: Vec<&str> = status.fans.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(ids, ["radiator", "pump", "case"]);

        let radiator = &status.fans[0];
        assert_eq!(radiator.percent, Some(38));
        assert_eq!(radiator.rpm, Some(1000));
        assert_eq!(radiator.mode, Some(FanMode::Curve));
        assert_eq!(radiator.min_percent, Some(25));
        assert_eq!(radiator.sensor.as_deref(), Some("cpu/temp1"));

        let pump = &status.fans[1];
        assert_eq!(pump.role, Role::Pump);
        assert_eq!(pump.rpm, Some(2000));
        assert_eq!(
            (pump.percent, pump.mode, pump.sensor.clone()),
            (None, None, None)
        );

        let case = &status.fans[2];
        assert_eq!(case.mode, Some(FanMode::Auto));
        assert_eq!(case.percent, None);
        assert_eq!(case.rpm, Some(3000));
    }

    #[test]
    fn rpm_input_picks_another_tachometer() {
        let mut config = config();
        let pump = config.fans.iter_mut().find(|f| f.id == "pump").unwrap();
        pump.rpm_input = Some(3);
        let snapshot = ControlSnapshot {
            chips: vec![board()],
            ..ControlSnapshot::default()
        };
        let status = build_status(&config, &snapshot, &GpuReading::default());
        let pump = status.fans.iter().find(|f| f.id == "pump").unwrap();
        assert_eq!(pump.rpm, Some(3000));

        let mut config = self::config();
        config.fans[0].rpm_input = Some(5);
        let status = build_status(&config, &snapshot, &GpuReading::default());
        assert_eq!(status.fans[0].rpm, None);
    }

    #[test]
    fn missing_chip_means_no_rpm() {
        let status = build_status(
            &config(),
            &ControlSnapshot::default(),
            &GpuReading::default(),
        );
        assert!(status.fans.iter().all(|f| f.rpm.is_none()));
    }
}
