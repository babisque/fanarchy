//! The setup wizard's rules: `list_outputs` and `apply_setup`.
//!
//! Pure functions only. [`build_config`] turns an `apply_setup` request
//! into a new config; the control loop then checks it against the
//! hardware with the same [`config::resolve`] as the config file, and
//! re-plans the controlled outputs.
//!
//! Only the `fans` list (fans and pumps) can change through the socket.
//! `chips`, `safety` and `version` always come from the running config,
//! and a pump can only be removed by root, by editing the file.

use fanarchy_protocol::{
    AssignedKind, Assignment, OutputInfo, OutputList, OutputMode, SetupFan, SetupPump,
};

use crate::config::{self, Config, FanConfig, Role, ValidationError};
use crate::curve::CurvePoint;
use crate::hwmon::{Chip, OutputRef, pwm_to_percent};

/// The curve a newly added fan starts with. Defined here only: the plugin
/// receives it with `list_outputs`.
pub const STARTER_CURVE: [CurvePoint; 4] =
    [(40.0, 30.0), (60.0, 50.0), (75.0, 80.0), (85.0, 100.0)];

/// Why an `apply_setup` request was refused. Nothing changes then.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SetupError {
    #[error("pump '{0}' can only be removed by editing the config file as root")]
    PumpRemoved(String),
    #[error("invalid setup: {}", .0.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))]
    Invalid(Vec<ValidationError>),
}

/// [`STARTER_CURVE`] with every point raised to at least `min_percent`.
pub fn starter_curve(min_percent: u8) -> Vec<CurvePoint> {
    let floor = f32::from(min_percent);
    STARTER_CURVE
        .iter()
        .map(|&(temp, percent)| (temp, percent.max(floor)))
        .collect()
}

/// Every `pwmN` output on the chips declared in `config.chips`, in alias
/// order. Chips that are missing or ambiguous are left out (`resolve`
/// reports them).
pub fn list_outputs(config: &Config, chips: &[Chip]) -> OutputList {
    let mut outputs = Vec::new();
    for (alias, name) in &config.chips {
        let Ok(chip) = config::find_chip(chips, alias, name) else {
            continue;
        };
        for pwm in &chip.pwms {
            let output = OutputRef {
                chip_path: chip.path.clone(),
                index: pwm.index,
            };
            let tachometer = chip.fans.iter().find(|fan| fan.index == pwm.index);
            outputs.push(OutputInfo {
                chip: alias.clone(),
                pwm: pwm.index,
                path: output.pwm_path().display().to_string(),
                mode: pwm.mode.map(OutputMode::from),
                percent: pwm.value.map(pwm_to_percent),
                rpm: tachometer.and_then(|fan| fan.rpm),
                rpm_input: tachometer.map(|fan| fan.index),
                assigned: assignment(config, chips, &output),
            });
        }
    }
    OutputList {
        starter_curve: STARTER_CURVE.to_vec(),
        outputs,
    }
}

/// The config entry on this physical output, if any.
fn assignment(config: &Config, chips: &[Chip], output: &OutputRef) -> Assignment {
    let entry = config.fans.iter().find(|fan| {
        fan.pwm == output.index
            && config
                .chips
                .get(&fan.chip)
                .and_then(|name| config::find_chip(chips, &fan.chip, name).ok())
                .is_some_and(|chip| chip.path == output.chip_path)
    });
    match entry {
        Some(fan) => Assignment {
            kind: match fan.role {
                Role::Fan => AssignedKind::Fan,
                Role::Pump => AssignedKind::Pump,
            },
            id: Some(fan.id.clone()),
        },
        None => Assignment {
            kind: AssignedKind::None,
            id: None,
        },
    }
}

/// The config an `apply_setup` request asks for, checked against every
/// schema rule. `current` itself is left untouched.
///
/// - Every pump of `current` must be in `pumps` with the same id, chip
///   and `pwm`; it is kept exactly as in `current`.
/// - No fan may reuse a pump's id or its chip and `pwm`.
/// - A fan without a curve gets [`starter_curve`]. Smoothing settings
///   (not part of the request) are kept from the fan with the same id.
pub fn build_config(
    current: &Config,
    pumps: &[SetupPump],
    fans: &[SetupFan],
) -> Result<Config, SetupError> {
    let current_pumps: Vec<&FanConfig> = current
        .fans
        .iter()
        .filter(|fan| fan.role == Role::Pump)
        .collect();

    for pump in &current_pumps {
        let kept = pumps
            .iter()
            .any(|p| p.id == pump.id && p.chip == pump.chip && p.pwm == pump.pwm);
        if !kept {
            return Err(SetupError::PumpRemoved(pump.id.clone()));
        }
    }
    for fan in fans {
        if let Some(pump) = current_pumps
            .iter()
            .find(|pump| pump.id == fan.id || (pump.chip == fan.chip && pump.pwm == fan.pwm))
        {
            return Err(SetupError::PumpRemoved(pump.id.clone()));
        }
    }

    let mut entries = Vec::with_capacity(pumps.len() + fans.len());
    for pump in pumps {
        let existing = current_pumps.iter().find(|p| p.id == pump.id);
        entries.push(match existing {
            Some(existing) => (*existing).clone(),
            None => new_pump(pump),
        });
    }
    for fan in fans {
        let previous = current
            .fans
            .iter()
            .find(|f| f.id == fan.id && f.role == Role::Fan);
        entries.push(fan_entry(fan, previous));
    }

    let mut config = current.clone();
    config.fans = entries;
    config::validate(&config).map_err(SetupError::Invalid)?;
    Ok(config)
}

fn new_pump(pump: &SetupPump) -> FanConfig {
    FanConfig {
        id: pump.id.clone(),
        label: pump.label.clone(),
        chip: pump.chip.clone(),
        pwm: pump.pwm,
        rpm_input: None,
        role: Role::Pump,
        sensor: None,
        curve: None,
        min_percent: None,
        hysteresis_c: None,
        step_up_percent: None,
        step_down_percent: None,
    }
}

fn fan_entry(fan: &SetupFan, previous: Option<&FanConfig>) -> FanConfig {
    FanConfig {
        id: fan.id.clone(),
        label: fan.label.clone(),
        chip: fan.chip.clone(),
        pwm: fan.pwm,
        // The default tachometer is the same N as the pwm: keep the file
        // free of a redundant field.
        rpm_input: fan.rpm_input.filter(|&input| input != fan.pwm),
        role: Role::Fan,
        sensor: Some(fan.sensor.clone()),
        curve: Some(
            fan.curve
                .clone()
                .unwrap_or_else(|| starter_curve(fan.min_percent)),
        ),
        min_percent: Some(fan.min_percent),
        hysteresis_c: previous.and_then(|f| f.hysteresis_c),
        step_up_percent: previous.and_then(|f| f.step_up_percent),
        step_down_percent: previous.and_then(|f| f.step_down_percent),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::path::PathBuf;

    use super::*;
    use crate::hwmon::{Fan, PwmMode, PwmOutput, TempSensor};

    const CURRENT: &str = r#"{
        "version": 1,
        "chips": { "board": "it8689", "cpu": "k10temp" },
        "fans": [
            { "id": "radiator", "label": "Radiator", "chip": "board", "pwm": 1, "role": "fan",
              "sensor": "cpu/temp1", "curve": [[40, 30], [85, 100]], "min_percent": 25,
              "hysteresis_c": 3, "step_up_percent": 20, "step_down_percent": 5 },
            { "id": "pump", "label": "AIO pump", "chip": "board", "pwm": 2, "role": "pump" }
        ],
        "safety": { "critical_temp_c": 80, "sensor_fail_percent": 90, "cpu_sensor": "cpu/temp1" }
    }"#;

    const EMPTY: &str = r#"{
        "version": 1,
        "chips": { "board": "it8689", "cpu": "k10temp" },
        "fans": [],
        "safety": { "critical_temp_c": 85, "sensor_fail_percent": 100, "cpu_sensor": "cpu/temp1" }
    }"#;

    fn current() -> Config {
        config::parse(CURRENT).unwrap()
    }

    fn pump(id: &str, pwm: u32) -> SetupPump {
        SetupPump {
            id: id.into(),
            label: "AIO pump".into(),
            chip: "board".into(),
            pwm,
        }
    }

    fn fan(id: &str, pwm: u32) -> SetupFan {
        SetupFan {
            id: id.into(),
            label: "Case fans".into(),
            chip: "board".into(),
            pwm,
            sensor: "cpu/temp1".into(),
            min_percent: 20,
            rpm_input: None,
            curve: None,
        }
    }

    fn chip(dir: &str, name: &str, pwms: u32, tachs: u32) -> Chip {
        Chip {
            name: name.into(),
            path: PathBuf::from(format!("/sys/class/hwmon/{dir}")),
            temps: vec![TempSensor {
                index: 1,
                label: None,
                celsius: Some(40.0),
            }],
            fans: (1..=tachs)
                .map(|index| Fan {
                    index,
                    label: None,
                    rpm: Some(if index == 2 { 0 } else { 800 + index }),
                })
                .collect(),
            pwms: (1..=pwms)
                .map(|index| PwmOutput {
                    index,
                    value: Some(102),
                    mode: Some(if index == 1 {
                        PwmMode::Manual
                    } else {
                        PwmMode::Automatic(2)
                    }),
                })
                .collect(),
        }
    }

    #[test]
    fn starter_curve_is_clamped_to_the_minimum() {
        assert_eq!(starter_curve(20), STARTER_CURVE.to_vec());
        assert_eq!(
            starter_curve(55),
            vec![(40.0, 55.0), (60.0, 55.0), (75.0, 80.0), (85.0, 100.0)]
        );
    }

    #[test]
    fn builds_the_spec_example_from_an_empty_config() {
        let empty = config::parse(EMPTY).unwrap();
        let mut case = fan("case", 3);
        case.rpm_input = Some(3);
        let config = build_config(&empty, &[pump("pump", 2)], &[fan("radiator", 1), case]).unwrap();

        assert_eq!(config.chips, empty.chips);
        assert_eq!(config.safety, empty.safety);
        let ids: Vec<_> = config
            .fans
            .iter()
            .map(|f| (f.id.as_str(), f.role))
            .collect();
        assert_eq!(
            ids,
            [
                ("pump", Role::Pump),
                ("radiator", Role::Fan),
                ("case", Role::Fan)
            ]
        );
        let case = &config.fans[2];
        assert_eq!(case.rpm_input, None, "the default tachometer is not stored");
        assert_eq!(case.curve, Some(starter_curve(20)));
        assert_eq!(case.min_percent, Some(20));
        assert_eq!(case.sensor.as_deref(), Some("cpu/temp1"));
    }

    #[test]
    fn keeps_chips_safety_and_existing_pumps() {
        let current = current();
        let mut renamed = pump("pump", 2);
        renamed.label = "Renamed".into();
        let config = build_config(&current, &[renamed], &[fan("radiator", 1)]).unwrap();
        assert_eq!(config.chips, current.chips);
        assert_eq!(config.safety, current.safety);
        assert_eq!(config.version, current.version);
        assert_eq!(config.fans[0], current.fans[1], "pump kept verbatim");
    }

    #[test]
    fn keeps_smoothing_settings_and_custom_curves() {
        let current = current();
        let mut radiator = fan("radiator", 1);
        radiator.curve = Some(vec![(30.0, 40.0), (70.0, 100.0)]);
        radiator.rpm_input = Some(3);
        let config = build_config(&current, &[pump("pump", 2)], &[radiator]).unwrap();
        let radiator = &config.fans[1];
        assert_eq!(radiator.hysteresis_c, Some(3.0));
        assert_eq!(radiator.step_up_percent, Some(20.0));
        assert_eq!(radiator.step_down_percent, Some(5.0));
        assert_eq!(radiator.curve, Some(vec![(30.0, 40.0), (70.0, 100.0)]));
        assert_eq!(radiator.rpm_input, Some(3));

        // A new id starts with the defaults.
        let config = build_config(&current, &[pump("pump", 2)], &[fan("other", 1)]).unwrap();
        assert_eq!(config.fans[1].hysteresis_c, None);
    }

    #[test]
    fn pumps_can_only_be_removed_by_root() {
        let current = current();
        let expected = SetupError::PumpRemoved("pump".into());
        assert_eq!(
            expected.to_string(),
            "pump 'pump' can only be removed by editing the config file as root"
        );

        // Left out.
        assert_eq!(build_config(&current, &[], &[]), Err(expected.clone()));
        // Moved to another output.
        assert_eq!(
            build_config(&current, &[pump("pump", 4)], &[]),
            Err(expected.clone())
        );
        // Renamed: the old id is gone.
        assert_eq!(
            build_config(&current, &[pump("aio", 2)], &[]),
            Err(expected.clone())
        );
        // Turned into a fan, by id or by output.
        assert_eq!(
            build_config(&current, &[pump("pump", 2)], &[fan("pump", 4)]),
            Err(expected.clone())
        );
        assert_eq!(
            build_config(&current, &[pump("pump", 2)], &[fan("case", 2)]),
            Err(expected.clone())
        );
        assert_eq!(
            build_config(&current, &[], &[fan("pump", 2)]),
            Err(expected)
        );
    }

    #[test]
    fn new_pumps_can_be_added() {
        let current = current();
        let config = build_config(&current, &[pump("pump", 2), pump("pump2", 4)], &[]).unwrap();
        assert_eq!(config.fans[1].role, Role::Pump);
        assert_eq!(config.fans[1].pwm, 4);
        assert_eq!(config.fans[1].min_percent, None);

        // Two pumps can never share an id.
        let result = build_config(&current, &[pump("pump", 2), pump("pump", 4)], &[]);
        let error = result.unwrap_err().to_string();
        assert!(
            error.starts_with("invalid setup: fan id 'pump' is used more than once"),
            "{error}"
        );
    }

    #[test]
    fn breaks_no_schema_rule() {
        let current = current();
        let pumps = [pump("pump", 2)];

        let result = build_config(&current, &pumps, &[fan("case", 1), fan("case", 3)]);
        assert_eq!(
            result.unwrap_err().to_string(),
            "invalid setup: fan id 'case' is used more than once"
        );

        let mut unknown = fan("case", 3);
        unknown.chip = "gpu".into();
        let result = build_config(&current, &pumps, &[unknown]);
        assert_eq!(
            result.unwrap_err().to_string(),
            "invalid setup: fan 'case': unknown chip alias 'gpu'"
        );

        let mut sensor = fan("case", 3);
        sensor.sensor = "temp1".into();
        assert!(build_config(&current, &pumps, &[sensor]).is_err());

        let mut zero = fan("case", 3);
        zero.min_percent = 0;
        assert!(build_config(&current, &pumps, &[zero]).is_err());

        let mut above_fail = fan("case", 3);
        above_fail.min_percent = 95;
        assert!(build_config(&current, &pumps, &[above_fail]).is_err());

        let mut empty_id = fan("", 3);
        empty_id.label = String::new();
        assert!(build_config(&current, &pumps, &[empty_id]).is_err());
    }

    #[test]
    fn lists_every_output_on_the_configured_chips() {
        let config = current();
        let chips = [
            chip("hwmon2", "k10temp", 0, 0),
            chip("hwmon5", "it8689_61030507", 5, 3),
            chip("hwmon7", "nct6775", 2, 2),
        ];
        let list = list_outputs(&config, &chips);
        assert_eq!(list.starter_curve, STARTER_CURVE.to_vec());
        assert_eq!(list.outputs.len(), 5, "only chips named in the config");

        let first = &list.outputs[0];
        assert_eq!(first.chip, "board");
        assert_eq!(first.pwm, 1);
        assert_eq!(first.path, "/sys/class/hwmon/hwmon5/pwm1");
        assert_eq!(first.mode, Some(OutputMode::Manual));
        assert_eq!(first.percent, Some(40));
        assert_eq!(first.rpm, Some(801));
        assert_eq!(first.rpm_input, Some(1));
        assert_eq!(first.assigned.kind, AssignedKind::Fan);
        assert_eq!(first.assigned.id.as_deref(), Some("radiator"));

        let pump = &list.outputs[1];
        assert_eq!(pump.assigned.kind, AssignedKind::Pump);
        assert_eq!(pump.rpm, Some(0));
        assert_eq!(pump.mode, Some(OutputMode::Automatic));

        let free = &list.outputs[3];
        assert_eq!(free.pwm, 4);
        assert_eq!(free.rpm, None);
        assert_eq!(free.rpm_input, None, "no fan4_input");
        assert_eq!(free.assigned.kind, AssignedKind::None);
        assert_eq!(free.assigned.id, None);
    }

    #[test]
    fn missing_chips_list_nothing() {
        let list = list_outputs(&current(), &[chip("hwmon2", "k10temp", 0, 0)]);
        assert!(list.outputs.is_empty());
        assert_eq!(list.starter_curve.len(), 4);
    }
}
