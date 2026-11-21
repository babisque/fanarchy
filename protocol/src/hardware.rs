//! Hardware readings, as read by the daemon and sent in `status`.
//!
//! The daemon fills these from `/sys/class/hwmon` and `nvidia-smi`; the
//! CLI and the plugin only ever receive them over the socket.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One hwmon chip, identified by its `name` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chip {
    /// Contents of the chip's `name` file, e.g. `k10temp`.
    pub name: String,
    /// Directory the chip was read from. Valid for this boot only:
    /// the `hwmonN` index changes between boots, so never persist it.
    /// Never sent over the socket.
    #[serde(skip)]
    pub path: PathBuf,
    pub temps: Vec<TempSensor>,
    pub fans: Vec<Fan>,
    pub pwms: Vec<PwmOutput>,
}

/// A `tempN_input` sensor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TempSensor {
    pub index: u32,
    pub label: Option<String>,
    /// `None` when the file is missing, unreadable or out of range.
    pub celsius: Option<f32>,
}

/// A `fanN_input` tachometer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fan {
    pub index: u32,
    pub label: Option<String>,
    /// `None` when unavailable, `Some(0)` when the fan is stopped.
    pub rpm: Option<u32>,
}

/// A `pwmN` output and its `pwmN_enable` mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PwmOutput {
    pub index: u32,
    /// Raw duty cycle, 0–255. `None` when unavailable.
    pub value: Option<u8>,
    /// `None` when `pwmN_enable` is missing or unreadable.
    pub mode: Option<PwmMode>,
}

/// Meaning of `pwmN_enable`. Sent as the raw number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "u32", from = "u32")]
pub enum PwmMode {
    /// `0`: no control, fan at full speed.
    Disabled,
    /// `1`: manual control through `pwmN`.
    Manual,
    /// `2` or higher: automatic (BIOS / chip) control. Keeps the raw value.
    Automatic(u32),
}

impl PwmMode {
    /// The raw value of `pwmN_enable` for this mode.
    pub fn raw(self) -> u32 {
        match self {
            PwmMode::Disabled => 0,
            PwmMode::Manual => 1,
            PwmMode::Automatic(n) => n,
        }
    }
}

impl From<u32> for PwmMode {
    fn from(raw: u32) -> Self {
        match raw {
            0 => PwmMode::Disabled,
            1 => PwmMode::Manual,
            n => PwmMode::Automatic(n),
        }
    }
}

impl From<PwmMode> for u32 {
    fn from(mode: PwmMode) -> Self {
        mode.raw()
    }
}

/// Temperature of one GPU.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GpuTemp {
    pub index: u32,
    pub name: String,
    /// `None` when the driver reports `[N/A]` or an out-of-range value.
    pub celsius: Option<f32>,
}

/// Convert a raw PWM value (0–255) to a rounded percentage (0–100).
pub fn pwm_to_percent(value: u8) -> u8 {
    let percent = (u32::from(value) * 100 + 127) / 255;
    // Always <= 100, so the conversion cannot fail.
    u8::try_from(percent).unwrap_or(100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pwm_mode_is_sent_as_raw_number() {
        let cases = [
            (PwmMode::Disabled, "0"),
            (PwmMode::Manual, "1"),
            (PwmMode::Automatic(2), "2"),
            (PwmMode::Automatic(5), "5"),
        ];
        for (mode, json) in cases {
            assert_eq!(serde_json::to_string(&mode).unwrap(), json);
            assert_eq!(serde_json::from_str::<PwmMode>(json).unwrap(), mode);
        }
    }

    #[test]
    fn chip_path_is_not_sent() {
        let chip = Chip {
            name: "k10temp".into(),
            path: PathBuf::from("/sys/class/hwmon/hwmon2"),
            temps: Vec::new(),
            fans: Vec::new(),
            pwms: Vec::new(),
        };
        let json = serde_json::to_string(&chip).unwrap();
        assert_eq!(json, r#"{"name":"k10temp","temps":[],"fans":[],"pwms":[]}"#);
        let back: Chip = serde_json::from_str(&json).unwrap();
        assert_eq!(back.path, PathBuf::new());
    }
}
