//! Text tables for `fanarchyd status` and `fanarchyctl status`.
//!
//! Pure functions only: readings in, aligned text table out.

use crate::hardware::{Chip, GpuTemp, PwmMode, pwm_to_percent};
use crate::{FanMode, FanStatus, Override, Role};

/// Shown in the CHIP column for GPUs read through `nvidia-smi`.
const GPU_SOURCE: &str = "nvidia-smi";
const UNAVAILABLE: &str = "n/a";
const NO_LABEL: &str = "-";

/// One table row before alignment.
#[derive(Debug, Clone, PartialEq)]
struct Row {
    chip: String,
    sensor: String,
    label: String,
    value: String,
    state: String,
}

/// Render every reading as one aligned table.
///
/// `gpu_note` is printed under the table, e.g. when `nvidia-smi` failed.
pub fn render_status(chips: &[Chip], gpus: &[GpuTemp], gpu_note: Option<&str>) -> String {
    let mut rows: Vec<Row> = chips.iter().flat_map(chip_rows).collect();
    rows.extend(gpus.iter().map(gpu_row));

    let mut out = if rows.is_empty() {
        String::from("No sensors found.\n")
    } else {
        format_table(&rows)
    };
    if let Some(note) = gpu_note {
        out.push_str(&format!("GPU: {note}\n"));
    }
    out
}

fn chip_rows(chip: &Chip) -> Vec<Row> {
    let temps = chip.temps.iter().map(|t| Row {
        chip: chip.name.clone(),
        sensor: format!("temp{}", t.index),
        label: label_text(t.label.as_deref()),
        value: celsius_text(t.celsius),
        state: String::new(),
    });
    let fans = chip.fans.iter().map(|f| Row {
        chip: chip.name.clone(),
        sensor: format!("fan{}", f.index),
        label: label_text(f.label.as_deref()),
        value: rpm_text(f.rpm),
        state: if f.rpm == Some(0) {
            "stopped".to_string()
        } else {
            String::new()
        },
    });
    let pwms = chip.pwms.iter().map(|p| Row {
        chip: chip.name.clone(),
        sensor: format!("pwm{}", p.index),
        label: NO_LABEL.to_string(),
        value: percent_text(p.value),
        state: mode_text(p.mode),
    });
    temps.chain(fans).chain(pwms).collect()
}

fn gpu_row(gpu: &GpuTemp) -> Row {
    Row {
        chip: GPU_SOURCE.to_string(),
        sensor: format!("gpu{}", gpu.index),
        label: label_text(Some(&gpu.name)),
        value: celsius_text(gpu.celsius),
        state: String::new(),
    }
}

fn label_text(label: Option<&str>) -> String {
    label
        .filter(|l| !l.is_empty())
        .unwrap_or(NO_LABEL)
        .to_string()
}

fn celsius_text(celsius: Option<f32>) -> String {
    celsius.map_or_else(|| UNAVAILABLE.to_string(), |c| format!("{c:.1} °C"))
}

fn rpm_text(rpm: Option<u32>) -> String {
    rpm.map_or_else(|| UNAVAILABLE.to_string(), |r| format!("{r} RPM"))
}

fn percent_text(value: Option<u8>) -> String {
    value.map_or_else(
        || UNAVAILABLE.to_string(),
        |v| format!("{} %", pwm_to_percent(v)),
    )
}

fn mode_text(mode: Option<PwmMode>) -> String {
    match mode {
        Some(PwmMode::Disabled) => "full speed".to_string(),
        Some(PwmMode::Manual) => "manual".to_string(),
        Some(PwmMode::Automatic(_)) => "automatic".to_string(),
        None => "mode n/a".to_string(),
    }
}

fn width(text: &str) -> usize {
    text.chars().count()
}

fn format_table(rows: &[Row]) -> String {
    let header = Row {
        chip: "CHIP".to_string(),
        sensor: "SENSOR".to_string(),
        label: "LABEL".to_string(),
        value: "VALUE".to_string(),
        state: "STATE".to_string(),
    };
    let cells: Vec<Vec<String>> = std::iter::once(&header)
        .chain(rows.iter())
        .map(|row| {
            vec![
                row.chip.clone(),
                row.sensor.clone(),
                row.label.clone(),
                row.value.clone(),
                row.state.clone(),
            ]
        })
        .collect();
    align(&cells, &[3])
}

/// Align rows of cells into columns separated by two spaces.
///
/// Columns listed in `right` are right-aligned. The last column is not
/// padded and trailing spaces are trimmed.
fn align(rows: &[Vec<String>], right: &[usize]) -> String {
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let widths: Vec<usize> = (0..columns)
        .map(|col| {
            rows.iter()
                .filter_map(|row| row.get(col))
                .map(|cell| width(cell))
                .max()
                .unwrap_or(0)
        })
        .collect();

    let mut out = String::new();
    for row in rows {
        let mut line = String::new();
        for (col, cell) in row.iter().enumerate() {
            if col > 0 {
                line.push_str("  ");
            }
            let pad = " ".repeat(widths[col].saturating_sub(width(cell)));
            let last = col + 1 == row.len();
            if right.contains(&col) {
                line.push_str(&pad);
                line.push_str(cell);
            } else {
                line.push_str(cell);
                if !last {
                    line.push_str(&pad);
                }
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// Render the configured fans (one row per output, pumps included).
pub fn render_fans(fans: &[FanStatus]) -> String {
    if fans.is_empty() {
        return String::from("No fans configured.\n");
    }
    let header = ["FAN", "LABEL", "SENSOR", "TEMP", "RPM", "SPEED", "MODE"];
    let mut rows = vec![header.iter().map(|h| h.to_string()).collect::<Vec<_>>()];
    rows.extend(fans.iter().map(fan_cells));
    align(&rows, &[3, 4, 5])
}

fn fan_cells(fan: &FanStatus) -> Vec<String> {
    let speed = match (fan.role, fan.percent) {
        (Role::Pump, _) => NO_LABEL.to_string(),
        (Role::Fan, Some(percent)) => format!("{percent} %"),
        (Role::Fan, None) => "bios".to_string(),
    };
    vec![
        fan.id.clone(),
        label_text(Some(&fan.label)),
        label_text(fan.sensor.as_deref()),
        match fan.role {
            Role::Pump => NO_LABEL.to_string(),
            Role::Fan => celsius_text(fan.temp_c),
        },
        rpm_text(fan.rpm),
        speed,
        fan_mode_text(fan),
    ]
}

fn fan_mode_text(fan: &FanStatus) -> String {
    if fan.role == Role::Pump {
        return "pump (not controlled)".to_string();
    }
    let mode = fan.mode.map_or(UNAVAILABLE, FanMode::as_str);
    match fan.safety_override {
        Some(Override::Critical) => format!("{mode}, CPU critical"),
        Some(Override::SensorFailed) => format!("{mode}, sensor failed"),
        Some(Override::None) | None => mode.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::hardware::{Fan, PwmOutput, TempSensor};

    fn sample_chip() -> Chip {
        Chip {
            name: "k10temp".to_string(),
            path: PathBuf::from("/fake"),
            temps: vec![
                TempSensor {
                    index: 1,
                    label: Some("Tctl".to_string()),
                    celsius: Some(48.3),
                },
                TempSensor {
                    index: 2,
                    label: None,
                    celsius: None,
                },
            ],
            fans: vec![
                Fan {
                    index: 1,
                    label: None,
                    rpm: Some(1477),
                },
                Fan {
                    index: 2,
                    label: None,
                    rpm: Some(0),
                },
                Fan {
                    index: 3,
                    label: None,
                    rpm: None,
                },
            ],
            pwms: vec![
                PwmOutput {
                    index: 1,
                    value: Some(255),
                    mode: Some(PwmMode::Automatic(2)),
                },
                PwmOutput {
                    index: 2,
                    value: Some(128),
                    mode: Some(PwmMode::Manual),
                },
                PwmOutput {
                    index: 3,
                    value: None,
                    mode: None,
                },
            ],
        }
    }

    #[test]
    fn renders_aligned_table() {
        let gpus = vec![GpuTemp {
            index: 0,
            name: "GTX".to_string(),
            celsius: Some(46.0),
        }];
        let out = render_status(&[sample_chip()], &gpus, None);
        let expected = "\
CHIP        SENSOR  LABEL     VALUE  STATE
k10temp     temp1   Tctl    48.3 °C
k10temp     temp2   -           n/a
k10temp     fan1    -      1477 RPM
k10temp     fan2    -         0 RPM  stopped
k10temp     fan3    -           n/a
k10temp     pwm1    -         100 %  automatic
k10temp     pwm2    -          50 %  manual
k10temp     pwm3    -           n/a  mode n/a
nvidia-smi  gpu0    GTX     46.0 °C
";
        assert_eq!(out, expected);
    }

    #[test]
    fn empty_input_says_so() {
        assert_eq!(render_status(&[], &[], None), "No sensors found.\n");
    }

    #[test]
    fn appends_gpu_note() {
        let out = render_status(&[], &[], Some("nvidia-smi failed"));
        assert_eq!(out, "No sensors found.\nGPU: nvidia-smi failed\n");
    }

    #[test]
    fn disabled_mode_means_full_speed() {
        assert_eq!(mode_text(Some(PwmMode::Disabled)), "full speed");
    }

    fn fan_status(id: &str, role: Role) -> FanStatus {
        FanStatus {
            id: id.to_string(),
            label: id.to_uppercase(),
            role,
            sensor: None,
            temp_c: None,
            rpm: Some(1200),
            percent: None,
            min_percent: None,
            mode: None,
            safety_override: None,
        }
    }

    #[test]
    fn renders_fans_table() {
        let radiator = FanStatus {
            sensor: Some("cpu/temp1".into()),
            temp_c: Some(48.25),
            percent: Some(38),
            min_percent: Some(25),
            mode: Some(FanMode::Curve),
            safety_override: Some(Override::None),
            ..fan_status("radiator", Role::Fan)
        };
        let case = FanStatus {
            sensor: Some("cpu/temp1".into()),
            temp_c: Some(90.0),
            rpm: None,
            percent: Some(100),
            min_percent: Some(20),
            mode: Some(FanMode::Auto),
            safety_override: Some(Override::Critical),
            ..fan_status("case", Role::Fan)
        };
        let idle = FanStatus {
            mode: Some(FanMode::Auto),
            safety_override: Some(Override::None),
            ..fan_status("rear", Role::Fan)
        };
        let out = render_fans(&[radiator, fan_status("pump", Role::Pump), case, idle]);
        let expected = "\
FAN       LABEL     SENSOR        TEMP       RPM  SPEED  MODE
radiator  RADIATOR  cpu/temp1  48.2 °C  1200 RPM   38 %  curve
pump      PUMP      -                -  1200 RPM      -  pump (not controlled)
case      CASE      cpu/temp1  90.0 °C       n/a  100 %  auto, CPU critical
rear      REAR      -              n/a  1200 RPM   bios  auto
";
        assert_eq!(out, expected);
    }

    #[test]
    fn no_fans_says_so() {
        assert_eq!(render_fans(&[]), "No fans configured.\n");
    }
}
