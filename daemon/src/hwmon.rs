//! Hardware access through `/sys/class/hwmon`.
//!
//! This is the only module that touches hardware. Everything else goes
//! through the [`Hwmon`] trait so it can be tested against a fake `/sys`.
//! Chips are always found by their `name` file, never by `hwmonN` index.
//!
//! Writes only ever target `pwmN` and `pwmN_enable` of an [`OutputRef`],
//! and `pwmN` only accepts a [`Duty`] that passed the safety rules.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::safety::Duty;

/// Default location of the hwmon class directory.
pub const DEFAULT_HWMON_ROOT: &str = "/sys/class/hwmon";

/// Lowest temperature we accept as a real reading, in °C.
const MIN_VALID_CELSIUS: f32 = -40.0;
/// Highest temperature we accept as a real reading, in °C.
/// ITE chips report 127 / -128 for a disconnected thermistor.
const MAX_VALID_CELSIUS: f32 = 125.0;
/// Fan readings at or above this are treated as garbage.
const MAX_VALID_RPM: u32 = 30_000;

/// Errors that stop hwmon discovery entirely.
///
/// Individual missing or unreadable files are not errors: the affected
/// sensor is reported as unavailable instead.
#[derive(Debug, thiserror::Error)]
pub enum HwmonError {
    #[error("cannot list hwmon root {path}: {source}")]
    RootUnreadable {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot write {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// One hwmon chip, identified by its `name` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Chip {
    /// Contents of the chip's `name` file, e.g. `k10temp`.
    pub name: String,
    /// Directory the chip was read from. Valid for this boot only:
    /// the `hwmonN` index changes between boots, so never persist it.
    pub path: PathBuf,
    pub temps: Vec<TempSensor>,
    pub fans: Vec<Fan>,
    pub pwms: Vec<PwmOutput>,
}

/// A `tempN_input` sensor.
#[derive(Debug, Clone, PartialEq)]
pub struct TempSensor {
    pub index: u32,
    pub label: Option<String>,
    /// `None` when the file is missing, unreadable or out of range.
    pub celsius: Option<f32>,
}

/// A `fanN_input` tachometer.
#[derive(Debug, Clone, PartialEq)]
pub struct Fan {
    pub index: u32,
    pub label: Option<String>,
    /// `None` when unavailable, `Some(0)` when the fan is stopped.
    pub rpm: Option<u32>,
}

/// A `pwmN` output and its `pwmN_enable` mode.
#[derive(Debug, Clone, PartialEq)]
pub struct PwmOutput {
    pub index: u32,
    /// Raw duty cycle, 0–255. `None` when unavailable.
    pub value: Option<u8>,
    /// `None` when `pwmN_enable` is missing or unreadable.
    pub mode: Option<PwmMode>,
}

/// Meaning of `pwmN_enable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

/// One `pwmN` output on a chip found during discovery.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutputRef {
    /// [`Chip::path`] of the chip. Valid for this boot only.
    pub chip_path: PathBuf,
    /// The `N` in `pwmN`.
    pub index: u32,
}

impl OutputRef {
    pub fn pwm_path(&self) -> PathBuf {
        self.chip_path.join(format!("pwm{}", self.index))
    }

    pub fn enable_path(&self) -> PathBuf {
        self.chip_path.join(format!("pwm{}_enable", self.index))
    }
}

/// Thin abstraction over hwmon access.
pub trait Hwmon {
    /// Every chip found, sorted by name.
    fn chips(&self) -> Result<Vec<Chip>, HwmonError>;

    /// Write a duty cycle to `pwmN`.
    fn write_pwm(&self, output: &OutputRef, duty: Duty) -> Result<(), HwmonError>;

    /// Write a mode to `pwmN_enable`.
    fn write_enable(&self, output: &OutputRef, mode: PwmMode) -> Result<(), HwmonError>;
}

/// Real implementation reading from a hwmon class directory.
///
/// `*_label` files never change while a chip exists, so each one is read
/// once and cached: the control loop calls [`Hwmon::chips`] every tick.
#[derive(Debug)]
pub struct SysHwmon {
    root: PathBuf,
    labels: LabelCache,
}

impl SysHwmon {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            labels: LabelCache::default(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Default for SysHwmon {
    fn default() -> Self {
        Self::new(DEFAULT_HWMON_ROOT)
    }
}

impl Clone for SysHwmon {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            labels: LabelCache(Mutex::new(self.labels.lock().clone())),
        }
    }
}

/// Label file path → label (`None` when missing or empty).
#[derive(Debug, Default)]
struct LabelCache(Mutex<HashMap<PathBuf, Option<String>>>);

impl LabelCache {
    fn lock(&self) -> MutexGuard<'_, HashMap<PathBuf, Option<String>>> {
        // A poisoned cache still holds valid labels.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn get(&self, dir: &Path, prefix: &str, index: u32) -> Option<String> {
        let path = dir.join(format!("{prefix}{index}_label"));
        let mut cache = self.lock();
        if let Some(label) = cache.get(&path) {
            return label.clone();
        }
        let label = read_optional_attr(&path).filter(|l| !l.is_empty());
        cache.insert(path, label.clone());
        label
    }
}

impl Hwmon for SysHwmon {
    fn chips(&self) -> Result<Vec<Chip>, HwmonError> {
        let entries = fs::read_dir(&self.root).map_err(|source| HwmonError::RootUnreadable {
            path: self.root.clone(),
            source,
        })?;

        let mut chips = Vec::new();
        for entry in entries.flatten() {
            // `path()` keeps the symlink; reads below follow it.
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            match read_chip(&dir, &self.labels) {
                Some(chip) => chips.push(chip),
                None => {
                    tracing::warn!(path = %dir.display(), "hwmon entry has no readable name, skipping")
                }
            }
        }
        chips.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.path.cmp(&b.path)));
        Ok(chips)
    }

    fn write_pwm(&self, output: &OutputRef, duty: Duty) -> Result<(), HwmonError> {
        let path = output.pwm_path();
        tracing::debug!(path = %path.display(), value = duty.raw(), "write pwm");
        write_attr(&path, &duty.raw().to_string())
    }

    fn write_enable(&self, output: &OutputRef, mode: PwmMode) -> Result<(), HwmonError> {
        let path = output.enable_path();
        tracing::debug!(path = %path.display(), value = mode.raw(), "write pwm_enable");
        write_attr(&path, &mode.raw().to_string())
    }
}

impl HwmonError {
    /// The driver refused a write because the device is busy (`EBUSY`).
    /// The it87 driver returns this for `pwmN` while `pwmN_enable` is
    /// automatic.
    pub fn is_busy(&self) -> bool {
        match self {
            HwmonError::Write { source, .. } => source.kind() == std::io::ErrorKind::ResourceBusy,
            HwmonError::RootUnreadable { .. } => false,
        }
    }
}

/// Write a sysfs attribute. Never creates files: a missing attribute is
/// an error, exactly like on real sysfs.
fn write_attr(path: &Path, value: &str) -> Result<(), HwmonError> {
    let to_error = |source| HwmonError::Write {
        path: path.to_path_buf(),
        source,
    };
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .map_err(to_error)?;
    file.write_all(value.as_bytes()).map_err(to_error)
}

/// Read one chip directory. Returns `None` if it has no `name`.
fn read_chip(dir: &Path, labels: &LabelCache) -> Option<Chip> {
    let name = read_attr(&dir.join("name")).filter(|n| !n.is_empty())?;

    let mut temp_indices = Vec::new();
    let mut fan_indices = Vec::new();
    let mut pwm_indices = Vec::new();
    for file_name in list_file_names(dir) {
        match parse_attr_name(&file_name) {
            Some(Attr {
                kind: AttrKind::Temp,
                index,
                suffix: AttrSuffix::Input,
            }) => temp_indices.push(index),
            Some(Attr {
                kind: AttrKind::Fan,
                index,
                suffix: AttrSuffix::Input,
            }) => fan_indices.push(index),
            Some(Attr {
                kind: AttrKind::Pwm,
                index,
                suffix: AttrSuffix::Value,
            }) => pwm_indices.push(index),
            _ => {}
        }
    }
    temp_indices.sort_unstable();
    fan_indices.sort_unstable();
    pwm_indices.sort_unstable();

    let temps = temp_indices
        .into_iter()
        .map(|index| TempSensor {
            index,
            label: labels.get(dir, "temp", index),
            celsius: read_attr(&dir.join(format!("temp{index}_input")))
                .as_deref()
                .and_then(millidegrees_to_celsius),
        })
        .collect();

    let fans = fan_indices
        .into_iter()
        .map(|index| Fan {
            index,
            label: labels.get(dir, "fan", index),
            rpm: read_attr(&dir.join(format!("fan{index}_input")))
                .as_deref()
                .and_then(parse_rpm),
        })
        .collect();

    let pwms = pwm_indices
        .into_iter()
        .map(|index| PwmOutput {
            index,
            value: read_attr(&dir.join(format!("pwm{index}")))
                .as_deref()
                .and_then(parse_pwm),
            mode: read_attr(&dir.join(format!("pwm{index}_enable")))
                .as_deref()
                .and_then(parse_pwm_mode),
        })
        .collect();

    Some(Chip {
        name,
        path: dir.to_path_buf(),
        temps,
        fans,
        pwms,
    })
}

/// Names of the entries in a chip directory. Unreadable dirs yield nothing.
fn list_file_names(dir: &Path) -> Vec<String> {
    match fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(err) => {
            tracing::debug!(path = %dir.display(), %err, "cannot list chip directory");
            Vec::new()
        }
    }
}

/// Read an attribute that is often absent (labels). Logs at trace only.
fn read_optional_attr(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) => Some(text.trim().to_string()),
        Err(err) => {
            tracing::trace!(path = %path.display(), %err, "optional hwmon attribute missing");
            None
        }
    }
}

/// Read a sysfs attribute, trimmed. Any error means "unavailable".
fn read_attr(path: &Path) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(text) => Some(text.trim().to_string()),
        Err(err) => {
            tracing::debug!(path = %path.display(), %err, "cannot read hwmon attribute");
            None
        }
    }
}

/// Which kind of sensor a file belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrKind {
    Temp,
    Fan,
    Pwm,
}

/// What a file holds for its sensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttrSuffix {
    /// `tempN_input`, `fanN_input`.
    Input,
    /// `tempN_label`, `fanN_label`.
    Label,
    /// `pwmN` itself.
    Value,
    /// `pwmN_enable`.
    Enable,
}

/// A parsed hwmon attribute file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attr {
    pub kind: AttrKind,
    pub index: u32,
    pub suffix: AttrSuffix,
}

/// Parse a hwmon file name such as `temp3_input` or `pwm1_enable`.
///
/// Returns `None` for files this phase does not care about
/// (`in0_input`, `temp1_max`, `pwm1_auto_point1_temp`, ...).
pub fn parse_attr_name(file_name: &str) -> Option<Attr> {
    const PREFIXES: [(&str, AttrKind); 3] = [
        ("temp", AttrKind::Temp),
        ("fan", AttrKind::Fan),
        ("pwm", AttrKind::Pwm),
    ];
    let (kind, rest) = PREFIXES
        .iter()
        .find_map(|(prefix, kind)| file_name.strip_prefix(prefix).map(|rest| (*kind, rest)))?;

    let digits_end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let (digits, tail) = rest.split_at(digits_end);
    if digits.is_empty() {
        return None;
    }
    let index: u32 = digits.parse().ok()?;

    let suffix = match (kind, tail) {
        (AttrKind::Temp | AttrKind::Fan, "_input") => AttrSuffix::Input,
        (AttrKind::Temp | AttrKind::Fan, "_label") => AttrSuffix::Label,
        (AttrKind::Pwm, "") => AttrSuffix::Value,
        (AttrKind::Pwm, "_enable") => AttrSuffix::Enable,
        _ => return None,
    };
    Some(Attr {
        kind,
        index,
        suffix,
    })
}

/// Convert a raw `tempN_input` value (millidegrees Celsius) to °C.
///
/// Out-of-range values (e.g. -127 °C for a disconnected probe) are `None`.
pub fn millidegrees_to_celsius(raw: &str) -> Option<f32> {
    let milli: i64 = raw.trim().parse().ok()?;
    let celsius = milli as f32 / 1000.0;
    (MIN_VALID_CELSIUS..=MAX_VALID_CELSIUS)
        .contains(&celsius)
        .then_some(celsius)
}

/// Parse a raw `fanN_input` value. `Some(0)` means the fan is stopped.
pub fn parse_rpm(raw: &str) -> Option<u32> {
    let rpm: u32 = raw.trim().parse().ok()?;
    (rpm < MAX_VALID_RPM).then_some(rpm)
}

/// Parse a raw `pwmN` value (0–255).
pub fn parse_pwm(raw: &str) -> Option<u8> {
    raw.trim().parse().ok()
}

/// Parse a raw `pwmN_enable` value.
pub fn parse_pwm_mode(raw: &str) -> Option<PwmMode> {
    match raw.trim().parse::<u32>().ok()? {
        0 => Some(PwmMode::Disabled),
        1 => Some(PwmMode::Manual),
        n => Some(PwmMode::Automatic(n)),
    }
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
    fn parses_attr_names() {
        type Case = (&'static str, Option<(AttrKind, u32, AttrSuffix)>);
        let cases: &[Case] = &[
            ("temp1_input", Some((AttrKind::Temp, 1, AttrSuffix::Input))),
            (
                "temp12_label",
                Some((AttrKind::Temp, 12, AttrSuffix::Label)),
            ),
            ("fan3_input", Some((AttrKind::Fan, 3, AttrSuffix::Input))),
            ("fan2_label", Some((AttrKind::Fan, 2, AttrSuffix::Label))),
            ("pwm1", Some((AttrKind::Pwm, 1, AttrSuffix::Value))),
            ("pwm5_enable", Some((AttrKind::Pwm, 5, AttrSuffix::Enable))),
            ("temp1_max", None),
            ("temp1_type", None),
            ("fan1_min", None),
            ("pwm1_auto_point1_temp", None),
            ("pwm1_freq", None),
            ("in0_input", None),
            ("name", None),
            ("temp_input", None),
            ("pwm", None),
            ("fanX_input", None),
            ("pwm1_label", None),
            ("fan1_enable", None),
        ];
        for (input, expected) in cases {
            let got = parse_attr_name(input).map(|a| (a.kind, a.index, a.suffix));
            assert_eq!(got, *expected, "input: {input}");
        }
    }

    #[test]
    fn converts_millidegrees() {
        let cases: &[(&str, Option<f32>)] = &[
            ("48250", Some(48.25)),
            ("0", Some(0.0)),
            ("-5000", Some(-5.0)),
            ("125000", Some(125.0)),
            ("-40000", Some(-40.0)),
            ("-127000", None),
            ("-128000", None),
            ("127000", None),
            ("", None),
            ("abc", None),
            ("48.5", None),
            (" 34000\n", Some(34.0)),
        ];
        for (input, expected) in cases {
            assert_eq!(
                millidegrees_to_celsius(input),
                *expected,
                "input: {input:?}"
            );
        }
    }

    #[test]
    fn parses_rpm() {
        let cases: &[(&str, Option<u32>)] = &[
            ("1477", Some(1477)),
            ("0", Some(0)),
            ("29999", Some(29999)),
            ("30000", None),
            ("65535", None),
            ("-1", None),
            ("", None),
            ("fast", None),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_rpm(input), *expected, "input: {input:?}");
        }
    }

    #[test]
    fn parses_pwm_value() {
        let cases: &[(&str, Option<u8>)] = &[
            ("0", Some(0)),
            ("128", Some(128)),
            ("255", Some(255)),
            ("256", None),
            ("-1", None),
            ("", None),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_pwm(input), *expected, "input: {input:?}");
        }
    }

    #[test]
    fn parses_pwm_mode() {
        let cases: &[(&str, Option<PwmMode>)] = &[
            ("0", Some(PwmMode::Disabled)),
            ("1", Some(PwmMode::Manual)),
            ("2", Some(PwmMode::Automatic(2))),
            ("5", Some(PwmMode::Automatic(5))),
            ("-1", None),
            ("", None),
            ("auto", None),
        ];
        for (input, expected) in cases {
            assert_eq!(parse_pwm_mode(input), *expected, "input: {input:?}");
        }
    }

    #[test]
    fn detects_busy_write_errors() {
        let write_error = |source| HwmonError::Write {
            path: PathBuf::from("/fake/pwm1"),
            source,
        };
        // EBUSY = 16 on Linux.
        assert!(write_error(std::io::Error::from_raw_os_error(16)).is_busy());
        assert!(!write_error(std::io::Error::from_raw_os_error(13)).is_busy());
        assert!(!write_error(std::io::Error::other("boom")).is_busy());
        let root = HwmonError::RootUnreadable {
            path: PathBuf::from("/fake"),
            source: std::io::Error::from_raw_os_error(16),
        };
        assert!(!root.is_busy());
    }

    #[test]
    fn pwm_mode_raw_round_trips() {
        for raw in ["0", "1", "2", "5"] {
            let mode = parse_pwm_mode(raw).unwrap();
            assert_eq!(mode.raw().to_string(), raw);
        }
    }

    #[test]
    fn converts_pwm_to_percent() {
        let cases: &[(u8, u8)] = &[
            (0, 0),
            (1, 0),
            (2, 1),
            (128, 50),
            (191, 75),
            (254, 100),
            (255, 100),
        ];
        for (input, expected) in cases {
            assert_eq!(pwm_to_percent(*input), *expected, "input: {input}");
        }
    }
}
