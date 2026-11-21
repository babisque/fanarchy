//! Config loading, validation and schema (v1).
//!
//! The config is checked in two stages:
//!
//! 1. [`validate`]: the file on its own (schema rules). Every error is
//!    collected so `check-config` can report them all at once.
//! 2. [`resolve`]: the config against the detected hwmon chips, producing
//!    a [`Plan`] with the outputs the daemon may control. Only `role: fan`
//!    outputs ever end up in a plan.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use fanarchy_protocol::Role;

use crate::curve::CurvePoint;
use crate::hwmon::{Chip, OutputRef, PwmMode};
use crate::safety::MIN_PERCENT_FLOOR;

/// Location of the daemon config file. Part of the public contract.
pub const CONFIG_PATH: &str = "/etc/fanarchy/config.json";

/// The only schema version this daemon understands.
pub const CONFIG_VERSION: u32 = 1;

pub const DEFAULT_CRITICAL_TEMP_C: f32 = 85.0;
pub const DEFAULT_SENSOR_FAIL_PERCENT: u8 = 100;
pub const DEFAULT_HYSTERESIS_C: f32 = 2.0;
pub const DEFAULT_STEP_UP_PERCENT: f32 = 10.0;
pub const DEFAULT_STEP_DOWN_PERCENT: f32 = 3.0;

/// `check-config` warns when a fan's `min_percent` is below this, since
/// many fans stall at low duty cycles.
pub const STALL_WARNING_PERCENT: u8 = 20;

const CRITICAL_TEMP_RANGE: (f32, f32) = (50.0, 100.0);
const CURVE_TEMP_RANGE: (f32, f32) = (0.0, 120.0);
const HYSTERESIS_RANGE: (f32, f32) = (0.0, 10.0);
const STEP_RANGE: (f32, f32) = (1.0, 100.0);

/// The whole config file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    /// Alias → hwmon chip name, e.g. `"board": "it8689"`.
    pub chips: BTreeMap<String, String>,
    pub fans: Vec<FanConfig>,
    pub safety: SafetyConfig,
}

/// One `pwmN` output: a controlled fan or a pump that is never touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FanConfig {
    pub id: String,
    pub label: String,
    /// A key of [`Config::chips`].
    pub chip: String,
    /// The `N` in `pwmN`.
    pub pwm: u32,
    pub role: Role,
    /// `"<chip alias>/tempN"`. Required for fans, forbidden for pumps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sensor: Option<String>,
    /// `[temp_c, percent]` points. Required for fans, forbidden for pumps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curve: Option<Vec<CurvePoint>>,
    /// Required for fans, forbidden for pumps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_percent: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hysteresis_c: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_up_percent: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_down_percent: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyConfig {
    #[serde(default = "default_critical_temp_c")]
    pub critical_temp_c: f32,
    #[serde(default = "default_sensor_fail_percent")]
    pub sensor_fail_percent: u8,
    /// `"<chip alias>/tempN"` checked against `critical_temp_c`.
    pub cpu_sensor: String,
}

fn default_critical_temp_c() -> f32 {
    DEFAULT_CRITICAL_TEMP_C
}

fn default_sensor_fail_percent() -> u8 {
    DEFAULT_SENSOR_FAIL_PERCENT
}

/// A parsed `"<chip alias>/tempN"` reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SensorRef {
    pub chip_alias: String,
    pub index: u32,
}

/// Parse `"<chip alias>/tempN"`. Returns `None` when malformed.
pub fn parse_sensor_ref(text: &str) -> Option<SensorRef> {
    let (alias, sensor) = text.split_once('/')?;
    let digits = sensor.strip_prefix("temp")?;
    if alias.is_empty() || digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let index: u32 = digits.parse().ok()?;
    (index >= 1).then(|| SensorRef {
        chip_alias: alias.to_string(),
        index,
    })
}

/// Errors loading the config file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read config {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse config: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("invalid config:\n{}", bullet_list(.0))]
    Invalid(Vec<ValidationError>),
    #[error("cannot save config {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl ConfigError {
    /// The error on one line, for socket replies.
    pub fn one_line(&self) -> String {
        match self {
            ConfigError::Invalid(errors) => format!("invalid config: {}", semicolon_list(errors)),
            other => other.to_string(),
        }
    }
}

/// One rule the config file breaks.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ValidationError {
    #[error("unsupported config version {0} (expected {CONFIG_VERSION})")]
    UnsupportedVersion(u32),
    #[error("chip alias must not be empty")]
    EmptyChipAlias,
    #[error("chip alias '{0}' has an empty chip name")]
    EmptyChipName(String),
    #[error("fan #{0} has an empty id")]
    EmptyFanId(usize),
    #[error("fan id '{0}' is used more than once")]
    DuplicateFanId(String),
    #[error("fan '{fan}': unknown chip alias '{alias}'")]
    UnknownChip { fan: String, alias: String },
    #[error("fan '{fan}': pwm must be 1 or higher")]
    InvalidPwmIndex { fan: String },
    #[error("fans '{first}' and '{second}' both use {chip}/pwm{pwm}")]
    DuplicatePwm {
        chip: String,
        pwm: u32,
        first: String,
        second: String,
    },
    #[error("pump '{fan}' must not have '{field}' (pumps are never controlled)")]
    PumpHasControl { fan: String, field: &'static str },
    #[error("fan '{fan}' is missing '{field}'")]
    MissingField { fan: String, field: &'static str },
    #[error("{context}: invalid sensor '{value}' (expected '<chip alias>/tempN')")]
    InvalidSensor { context: String, value: String },
    #[error("{context}: sensor '{value}' uses unknown chip alias '{alias}'")]
    UnknownSensorChip {
        context: String,
        value: String,
        alias: String,
    },
    #[error("fan '{fan}': curve needs at least 2 points")]
    CurveTooShort { fan: String },
    #[error(
        "fan '{fan}': curve point [{temp}, {percent}] is out of range (temp 0–120 °C, 0–100 %)"
    )]
    CurvePointOutOfRange {
        fan: String,
        temp: f32,
        percent: f32,
    },
    #[error("fan '{fan}': curve temperatures must be strictly increasing")]
    CurveTempsNotIncreasing { fan: String },
    #[error("fan '{fan}': curve percentages must not decrease")]
    CurvePercentDecreasing { fan: String },
    #[error("fan '{fan}': min_percent {value} is out of range ({MIN_PERCENT_FLOOR}–100)")]
    MinPercentOutOfRange { fan: String, value: u8 },
    #[error("fan '{fan}': {field} {value} is out of range ({min}–{max})")]
    SettingOutOfRange {
        fan: String,
        field: &'static str,
        value: f32,
        min: f32,
        max: f32,
    },
    #[error("safety.critical_temp_c {0} is out of range (50–100)")]
    CriticalTempOutOfRange(f32),
    #[error("safety.sensor_fail_percent {0} is out of range (1–100)")]
    SensorFailPercentOutOfRange(u8),
    #[error("safety.sensor_fail_percent {value} is below fan '{fan}' min_percent {min}")]
    SensorFailBelowMin { fan: String, value: u8, min: u8 },
}

/// Something worth pointing out that does not make the config invalid.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ConfigWarning {
    #[error(
        "fan '{fan}': min_percent {value} is below {STALL_WARNING_PERCENT} %; many fans stall that low"
    )]
    LowMinPercent { fan: String, value: u8 },
}

/// Errors matching the config against the detected hardware.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ResolveError {
    #[error("chip '{alias}': no hwmon chip named '{name}' or '{name}_*'")]
    ChipNotFound { alias: String, name: String },
    #[error("chip '{alias}': '{name}' matches more than one chip: {}", .matches.join(", "))]
    ChipAmbiguous {
        alias: String,
        name: String,
        matches: Vec<String>,
    },
    #[error("fan '{fan}': {chip} has no pwm{pwm}")]
    PwmNotFound { fan: String, chip: String, pwm: u32 },
    #[error("fan '{fan}': cannot read pwm{pwm}_enable on {chip}, so it could not be restored")]
    EnableUnreadable { fan: String, chip: String, pwm: u32 },
    #[error("sensor '{sensor}' not found")]
    SensorNotFound { sensor: String },
    #[error("{0}")]
    Invalid(ValidationError),
}

fn semicolon_list<T: fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

fn bullet_list<T: fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .map(|item| format!("  - {item}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse a config without validating it.
pub fn parse(text: &str) -> Result<Config, ConfigError> {
    Ok(serde_json::from_str(text)?)
}

/// Read a config file without validating it.
pub fn read(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    parse(&text)
}

/// Read and validate a config file.
pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let config = read(path)?;
    validate(&config).map_err(ConfigError::Invalid)?;
    Ok(config)
}

/// Write `config` to `path` atomically.
///
/// The JSON goes to `<path>.tmp` in the same directory, is flushed to
/// disk and then renamed over `path`, so readers see either the old or
/// the new file, never a partial one. The file keeps its permissions
/// (`0644` for a new file).
pub fn save_atomic(path: &Path, config: &Config) -> Result<(), ConfigError> {
    let to_error = |source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    };
    let mut text =
        serde_json::to_string_pretty(config).map_err(|err| to_error(std::io::Error::other(err)))?;
    text.push('\n');

    let mode = fs::metadata(path)
        .map(|meta| meta.permissions().mode() & 0o777)
        .unwrap_or(0o644);
    let tmp = tmp_path(path);
    let result = write_then_rename(&tmp, path, text.as_bytes(), mode);
    if result.is_err() {
        // Best effort: never leave a stray temp file behind.
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(to_error)
}

/// `<path>.tmp`, next to `path` so the rename stays on one filesystem.
pub fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

fn write_then_rename(tmp: &Path, path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(tmp)?;
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(tmp, path)?;
    // Make the rename itself durable.
    let dir = match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir,
        _ => Path::new("."),
    };
    File::open(dir)?.sync_all()
}

/// Why a change requested over the socket was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ChangeError {
    #[error("unknown fan id '{0}'")]
    UnknownFan(String),
    #[error("fan '{0}' is a pump and cannot be controlled")]
    Pump(String),
    #[error("{}", semicolon_list(.0))]
    Invalid(Vec<ValidationError>),
    #[error("percent is required for mode 'fixed'")]
    PercentRequired,
    #[error("percent is only allowed with mode 'fixed'")]
    PercentNotAllowed,
    #[error("fan '{fan}': percent {value} is out of range ({min}–100)")]
    PercentOutOfRange { fan: String, value: u8, min: u8 },
}

/// The `role: fan` entry with this id. Pumps and unknown ids are errors.
pub fn controllable_fan<'a>(config: &'a Config, id: &str) -> Result<&'a FanConfig, ChangeError> {
    let fan = config
        .fans
        .iter()
        .find(|fan| fan.id == id)
        .ok_or_else(|| ChangeError::UnknownFan(id.to_string()))?;
    match fan.role {
        Role::Fan => Ok(fan),
        Role::Pump => Err(ChangeError::Pump(id.to_string())),
    }
}

/// A copy of `config` with a new curve for fan `id`, checked against
/// every schema rule. `config` itself is left untouched.
pub fn with_curve(
    config: &Config,
    id: &str,
    curve: Vec<CurvePoint>,
) -> Result<Config, ChangeError> {
    controllable_fan(config, id)?;
    let mut changed = config.clone();
    for fan in changed.fans.iter_mut().filter(|fan| fan.id == id) {
        fan.curve = Some(curve.clone());
    }
    validate(&changed).map_err(ChangeError::Invalid)?;
    Ok(changed)
}

/// Whether two plans control exactly the same outputs. `reload` only
/// accepts a new config when this holds.
pub fn same_outputs(old: &Plan, new: &Plan) -> bool {
    let outputs = |plan: &Plan| -> HashSet<OutputRef> {
        plan.fans.iter().map(|fan| fan.output.clone()).collect()
    };
    old.fans.len() == new.fans.len() && outputs(old) == outputs(new)
}

/// Check every schema rule. Returns all broken rules at once.
pub fn validate(config: &Config) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    if config.version != CONFIG_VERSION {
        errors.push(ValidationError::UnsupportedVersion(config.version));
    }
    for (alias, name) in &config.chips {
        if alias.is_empty() {
            errors.push(ValidationError::EmptyChipAlias);
        }
        if name.is_empty() {
            errors.push(ValidationError::EmptyChipName(alias.clone()));
        }
    }

    let mut ids = HashSet::new();
    let mut outputs: HashMap<(&str, u32), &str> = HashMap::new();
    for (position, fan) in config.fans.iter().enumerate() {
        if fan.id.is_empty() {
            errors.push(ValidationError::EmptyFanId(position + 1));
        } else if !ids.insert(fan.id.as_str()) {
            errors.push(ValidationError::DuplicateFanId(fan.id.clone()));
        }
        if !config.chips.contains_key(&fan.chip) {
            errors.push(ValidationError::UnknownChip {
                fan: fan.id.clone(),
                alias: fan.chip.clone(),
            });
        }
        if fan.pwm == 0 {
            errors.push(ValidationError::InvalidPwmIndex {
                fan: fan.id.clone(),
            });
        }
        if let Some(first) = outputs.insert((fan.chip.as_str(), fan.pwm), fan.id.as_str()) {
            errors.push(ValidationError::DuplicatePwm {
                chip: fan.chip.clone(),
                pwm: fan.pwm,
                first: first.to_string(),
                second: fan.id.clone(),
            });
        }
        match fan.role {
            Role::Pump => validate_pump(fan, &mut errors),
            Role::Fan => validate_fan(fan, config, &mut errors),
        }
    }

    validate_safety(config, &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_pump(fan: &FanConfig, errors: &mut Vec<ValidationError>) {
    let control_fields = [
        ("sensor", fan.sensor.is_some()),
        ("curve", fan.curve.is_some()),
        ("min_percent", fan.min_percent.is_some()),
        ("hysteresis_c", fan.hysteresis_c.is_some()),
        ("step_up_percent", fan.step_up_percent.is_some()),
        ("step_down_percent", fan.step_down_percent.is_some()),
    ];
    for (field, present) in control_fields {
        if present {
            errors.push(ValidationError::PumpHasControl {
                fan: fan.id.clone(),
                field,
            });
        }
    }
}

fn validate_fan(fan: &FanConfig, config: &Config, errors: &mut Vec<ValidationError>) {
    let missing = |field| ValidationError::MissingField {
        fan: fan.id.clone(),
        field,
    };

    match &fan.sensor {
        Some(sensor) => validate_sensor(&format!("fan '{}'", fan.id), sensor, config, errors),
        None => errors.push(missing("sensor")),
    }
    match &fan.curve {
        Some(curve) => validate_curve(&fan.id, curve, errors),
        None => errors.push(missing("curve")),
    }
    match fan.min_percent {
        Some(min) if !(MIN_PERCENT_FLOOR..=100).contains(&min) => {
            errors.push(ValidationError::MinPercentOutOfRange {
                fan: fan.id.clone(),
                value: min,
            });
        }
        Some(_) => {}
        None => errors.push(missing("min_percent")),
    }

    let settings = [
        ("hysteresis_c", fan.hysteresis_c, HYSTERESIS_RANGE),
        ("step_up_percent", fan.step_up_percent, STEP_RANGE),
        ("step_down_percent", fan.step_down_percent, STEP_RANGE),
    ];
    for (field, value, (min, max)) in settings {
        if let Some(value) = value
            && !(min..=max).contains(&value)
        {
            errors.push(ValidationError::SettingOutOfRange {
                fan: fan.id.clone(),
                field,
                value,
                min,
                max,
            });
        }
    }
}

fn validate_sensor(context: &str, value: &str, config: &Config, errors: &mut Vec<ValidationError>) {
    match parse_sensor_ref(value) {
        None => errors.push(ValidationError::InvalidSensor {
            context: context.to_string(),
            value: value.to_string(),
        }),
        Some(sensor) if !config.chips.contains_key(&sensor.chip_alias) => {
            errors.push(ValidationError::UnknownSensorChip {
                context: context.to_string(),
                value: value.to_string(),
                alias: sensor.chip_alias,
            });
        }
        Some(_) => {}
    }
}

fn validate_curve(fan: &str, curve: &[CurvePoint], errors: &mut Vec<ValidationError>) {
    if curve.len() < 2 {
        errors.push(ValidationError::CurveTooShort {
            fan: fan.to_string(),
        });
    }
    for &(temp, percent) in curve {
        let temp_ok = (CURVE_TEMP_RANGE.0..=CURVE_TEMP_RANGE.1).contains(&temp);
        let percent_ok = (0.0..=100.0).contains(&percent);
        if !temp_ok || !percent_ok {
            errors.push(ValidationError::CurvePointOutOfRange {
                fan: fan.to_string(),
                temp,
                percent,
            });
        }
    }
    if curve.windows(2).any(|pair| pair[1].0 <= pair[0].0) {
        errors.push(ValidationError::CurveTempsNotIncreasing {
            fan: fan.to_string(),
        });
    }
    if curve.windows(2).any(|pair| pair[1].1 < pair[0].1) {
        errors.push(ValidationError::CurvePercentDecreasing {
            fan: fan.to_string(),
        });
    }
}

fn validate_safety(config: &Config, errors: &mut Vec<ValidationError>) {
    let safety = &config.safety;
    if !(CRITICAL_TEMP_RANGE.0..=CRITICAL_TEMP_RANGE.1).contains(&safety.critical_temp_c) {
        errors.push(ValidationError::CriticalTempOutOfRange(
            safety.critical_temp_c,
        ));
    }
    if !(1..=100).contains(&safety.sensor_fail_percent) {
        errors.push(ValidationError::SensorFailPercentOutOfRange(
            safety.sensor_fail_percent,
        ));
    }
    for fan in &config.fans {
        if let Some(min) = fan.min_percent
            && fan.role == Role::Fan
            && safety.sensor_fail_percent < min
        {
            errors.push(ValidationError::SensorFailBelowMin {
                fan: fan.id.clone(),
                value: safety.sensor_fail_percent,
                min,
            });
        }
    }
    validate_sensor("safety.cpu_sensor", &safety.cpu_sensor, config, errors);
}

/// Non-fatal remarks about a valid config.
pub fn warnings(config: &Config) -> Vec<ConfigWarning> {
    config
        .fans
        .iter()
        .filter(|fan| fan.role == Role::Fan)
        .filter_map(|fan| {
            let value = fan.min_percent?;
            (value < STALL_WARNING_PERCENT).then(|| ConfigWarning::LowMinPercent {
                fan: fan.id.clone(),
                value,
            })
        })
        .collect()
}

/// Whether a detected chip name matches a configured name.
///
/// Matches the exact name, or the name followed by `_<suffix>`: the
/// out-of-tree it87 driver reports e.g. `it8689_61030507`, and the suffix
/// may change between driver versions.
pub fn chip_name_matches(configured: &str, detected: &str) -> bool {
    match detected.strip_prefix(configured) {
        Some("") => true,
        Some(rest) => rest.starts_with('_'),
        None => false,
    }
}

/// Find the one chip a config alias points to.
pub fn find_chip<'a>(chips: &'a [Chip], alias: &str, name: &str) -> Result<&'a Chip, ResolveError> {
    let matches: Vec<&Chip> = chips
        .iter()
        .filter(|chip| chip_name_matches(name, &chip.name))
        .collect();
    match matches.as_slice() {
        [chip] => Ok(chip),
        [] => Err(ResolveError::ChipNotFound {
            alias: alias.to_string(),
            name: name.to_string(),
        }),
        many => Err(ResolveError::ChipAmbiguous {
            alias: alias.to_string(),
            name: name.to_string(),
            matches: many
                .iter()
                .map(|chip| format!("{} ({})", chip.name, chip.path.display()))
                .collect(),
        }),
    }
}

/// A temperature sensor located on a detected chip.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedSensor {
    /// The reference as written in the config, for logs.
    pub name: String,
    pub chip_path: PathBuf,
    pub index: u32,
}

/// A fan the daemon controls, with its settings and defaults applied.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlledFan {
    pub id: String,
    pub output: OutputRef,
    /// `pwmN_enable` as read before the daemon touched anything.
    pub original_mode: PwmMode,
    pub sensor: ResolvedSensor,
    pub curve: Vec<CurvePoint>,
    pub min_percent: u8,
    pub hysteresis_c: f32,
    pub step_up_percent: f32,
    pub step_down_percent: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SafetySettings {
    pub critical_temp_c: f32,
    pub sensor_fail_percent: u8,
}

/// Everything the control loop needs. Only `role: fan` outputs are here.
#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub fans: Vec<ControlledFan>,
    pub cpu_sensor: ResolvedSensor,
    pub safety: SafetySettings,
}

impl Plan {
    /// Whether the daemon may write to `output`.
    pub fn controls(&self, output: &OutputRef) -> bool {
        self.fans.iter().any(|fan| &fan.output == output)
    }
}

/// Match a validated config against the detected chips.
///
/// Fails when a chip, `pwmN`, readable `pwmN_enable` or sensor is
/// missing. Pump outputs are not looked up at all.
pub fn resolve(config: &Config, chips: &[Chip]) -> Result<Plan, Vec<ResolveError>> {
    let mut errors = Vec::new();
    let mut fans = Vec::new();

    for fan in config.fans.iter().filter(|fan| fan.role == Role::Fan) {
        let output = resolve_output(config, chips, fan);
        let sensor = fan
            .sensor
            .as_deref()
            .ok_or_else(|| {
                ResolveError::Invalid(ValidationError::MissingField {
                    fan: fan.id.clone(),
                    field: "sensor",
                })
            })
            .and_then(|sensor| resolve_sensor(config, chips, sensor));
        let curve = fan.curve.clone().ok_or_else(|| {
            ResolveError::Invalid(ValidationError::MissingField {
                fan: fan.id.clone(),
                field: "curve",
            })
        });
        let min_percent = fan.min_percent.ok_or_else(|| {
            ResolveError::Invalid(ValidationError::MissingField {
                fan: fan.id.clone(),
                field: "min_percent",
            })
        });
        match (output, sensor, curve, min_percent) {
            (Ok((output, original_mode)), Ok(sensor), Ok(curve), Ok(min_percent)) => {
                fans.push(ControlledFan {
                    id: fan.id.clone(),
                    output,
                    original_mode,
                    sensor,
                    curve,
                    min_percent,
                    hysteresis_c: fan.hysteresis_c.unwrap_or(DEFAULT_HYSTERESIS_C),
                    step_up_percent: fan.step_up_percent.unwrap_or(DEFAULT_STEP_UP_PERCENT),
                    step_down_percent: fan.step_down_percent.unwrap_or(DEFAULT_STEP_DOWN_PERCENT),
                });
            }
            (output, sensor, curve, min_percent) => {
                errors.extend(output.err());
                errors.extend(sensor.err());
                errors.extend(curve.err());
                errors.extend(min_percent.err());
            }
        }
    }

    let cpu_sensor = resolve_sensor(config, chips, &config.safety.cpu_sensor);
    match cpu_sensor {
        Ok(cpu_sensor) if errors.is_empty() => Ok(Plan {
            fans,
            cpu_sensor,
            safety: SafetySettings {
                critical_temp_c: config.safety.critical_temp_c,
                sensor_fail_percent: config.safety.sensor_fail_percent,
            },
        }),
        Ok(_) => Err(errors),
        Err(err) => {
            errors.push(err);
            Err(errors)
        }
    }
}

fn resolve_alias<'a>(
    config: &Config,
    chips: &'a [Chip],
    alias: &str,
) -> Result<&'a Chip, ResolveError> {
    let name = config.chips.get(alias).ok_or_else(|| {
        ResolveError::Invalid(ValidationError::UnknownChip {
            fan: String::new(),
            alias: alias.to_string(),
        })
    })?;
    find_chip(chips, alias, name)
}

fn resolve_output(
    config: &Config,
    chips: &[Chip],
    fan: &FanConfig,
) -> Result<(OutputRef, PwmMode), ResolveError> {
    let chip = resolve_alias(config, chips, &fan.chip)?;
    let pwm = chip
        .pwms
        .iter()
        .find(|pwm| pwm.index == fan.pwm)
        .ok_or_else(|| ResolveError::PwmNotFound {
            fan: fan.id.clone(),
            chip: chip.name.clone(),
            pwm: fan.pwm,
        })?;
    let mode = pwm.mode.ok_or_else(|| ResolveError::EnableUnreadable {
        fan: fan.id.clone(),
        chip: chip.name.clone(),
        pwm: fan.pwm,
    })?;
    let output = OutputRef {
        chip_path: chip.path.clone(),
        index: fan.pwm,
    };
    Ok((output, mode))
}

fn resolve_sensor(
    config: &Config,
    chips: &[Chip],
    text: &str,
) -> Result<ResolvedSensor, ResolveError> {
    let sensor = parse_sensor_ref(text).ok_or_else(|| {
        ResolveError::Invalid(ValidationError::InvalidSensor {
            context: "sensor".to_string(),
            value: text.to_string(),
        })
    })?;
    let chip = resolve_alias(config, chips, &sensor.chip_alias)?;
    if !chip.temps.iter().any(|temp| temp.index == sensor.index) {
        return Err(ResolveError::SensorNotFound {
            sensor: text.to_string(),
        });
    }
    Ok(ResolvedSensor {
        name: text.to_string(),
        chip_path: chip.path.clone(),
        index: sensor.index,
    })
}

/// Outputs `fanarchyd restore` sets back to automatic.
///
/// Best effort, for use after a crash: works on a config that was not
/// validated and does not need `pwmN_enable` to be readable. Only
/// `role: fan` outputs are returned, and an output that any pump also
/// claims is skipped, so a broken config can never reach a pump.
pub fn restore_outputs(
    config: &Config,
    chips: &[Chip],
) -> (Vec<(String, OutputRef)>, Vec<ResolveError>) {
    let pumps: HashSet<(&str, u32)> = config
        .fans
        .iter()
        .filter(|fan| fan.role == Role::Pump)
        .map(|fan| (fan.chip.as_str(), fan.pwm))
        .collect();

    let mut outputs = Vec::new();
    let mut errors = Vec::new();
    for fan in config.fans.iter().filter(|fan| fan.role == Role::Fan) {
        if pumps.contains(&(fan.chip.as_str(), fan.pwm)) {
            tracing::warn!(fan = %fan.id, "output is also claimed by a pump, not restoring it");
            continue;
        }
        let chip = match resolve_alias(config, chips, &fan.chip) {
            Ok(chip) => chip,
            Err(err) => {
                errors.push(err);
                continue;
            }
        };
        if chip.pwms.iter().any(|pwm| pwm.index == fan.pwm) {
            let output = OutputRef {
                chip_path: chip.path.clone(),
                index: fan.pwm,
            };
            outputs.push((fan.id.clone(), output));
        } else {
            errors.push(ResolveError::PwmNotFound {
                fan: fan.id.clone(),
                chip: chip.name.clone(),
                pwm: fan.pwm,
            });
        }
    }
    (outputs, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hwmon::{PwmOutput, TempSensor};

    const VALID: &str = r#"{
        "version": 1,
        "chips": { "board": "it8689", "cpu": "k10temp" },
        "fans": [
            { "id": "radiator", "label": "Radiator", "chip": "board", "pwm": 1, "role": "fan",
              "sensor": "cpu/temp1", "curve": [[40, 30], [60, 50], [85, 100]], "min_percent": 25 },
            { "id": "pump", "label": "AIO pump", "chip": "board", "pwm": 2, "role": "pump" },
            { "id": "case", "label": "Case", "chip": "board", "pwm": 3, "role": "fan",
              "sensor": "board/temp2", "curve": [[40, 25], [80, 100]], "min_percent": 20,
              "hysteresis_c": 3, "step_up_percent": 20, "step_down_percent": 5 }
        ],
        "safety": { "critical_temp_c": 85, "sensor_fail_percent": 100, "cpu_sensor": "cpu/temp1" }
    }"#;

    fn valid() -> Config {
        parse(VALID).unwrap()
    }

    fn fan_mut<'a>(config: &'a mut Config, id: &str) -> &'a mut FanConfig {
        config.fans.iter_mut().find(|f| f.id == id).unwrap()
    }

    #[test]
    fn valid_config_passes() {
        let config = valid();
        assert_eq!(validate(&config), Ok(()));
        assert_eq!(config.fans[0].curve.as_ref().unwrap()[1], (60.0, 50.0));
        assert_eq!(config.fans[1].role, Role::Pump);
    }

    #[test]
    fn safety_defaults_apply() {
        let config =
            parse(&VALID.replace(r#""critical_temp_c": 85, "sensor_fail_percent": 100, "#, ""))
                .unwrap();
        assert_eq!(config.safety.critical_temp_c, DEFAULT_CRITICAL_TEMP_C);
        assert_eq!(
            config.safety.sensor_fail_percent,
            DEFAULT_SENSOR_FAIL_PERCENT
        );
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let text = VALID.replace(r#""version": 1,"#, r#""version": 1, "colour": "red","#);
        assert!(matches!(parse(&text), Err(ConfigError::Parse(_))));
        let text = VALID.replace(r#""min_percent": 25"#, r#""min_percnt": 25"#);
        assert!(matches!(parse(&text), Err(ConfigError::Parse(_))));
    }

    #[test]
    fn unknown_role_is_rejected() {
        let text = VALID.replace(r#""role": "pump""#, r#""role": "aio""#);
        assert!(matches!(parse(&text), Err(ConfigError::Parse(_))));
    }

    #[test]
    fn rejects_broken_rules() {
        type Mutate = fn(&mut Config);
        let cases: Vec<(&str, Mutate, ValidationError)> = vec![
            (
                "version",
                |c| c.version = 2,
                ValidationError::UnsupportedVersion(2),
            ),
            (
                "empty chip name",
                |c| {
                    c.chips.insert("cpu".into(), String::new());
                },
                ValidationError::EmptyChipName("cpu".into()),
            ),
            (
                "empty id",
                |c| fan_mut(c, "case").id = String::new(),
                ValidationError::EmptyFanId(3),
            ),
            (
                "duplicate id",
                |c| fan_mut(c, "case").id = "radiator".into(),
                ValidationError::DuplicateFanId("radiator".into()),
            ),
            (
                "unknown chip",
                |c| fan_mut(c, "case").chip = "nope".into(),
                ValidationError::UnknownChip {
                    fan: "case".into(),
                    alias: "nope".into(),
                },
            ),
            (
                "pwm 0",
                |c| fan_mut(c, "case").pwm = 0,
                ValidationError::InvalidPwmIndex { fan: "case".into() },
            ),
            (
                "duplicate pwm between fans",
                |c| fan_mut(c, "case").pwm = 1,
                ValidationError::DuplicatePwm {
                    chip: "board".into(),
                    pwm: 1,
                    first: "radiator".into(),
                    second: "case".into(),
                },
            ),
            (
                "fan on the pump's pwm",
                |c| fan_mut(c, "case").pwm = 2,
                ValidationError::DuplicatePwm {
                    chip: "board".into(),
                    pwm: 2,
                    first: "pump".into(),
                    second: "case".into(),
                },
            ),
            (
                "pump with curve",
                |c| fan_mut(c, "pump").curve = Some(vec![(30.0, 50.0), (60.0, 100.0)]),
                ValidationError::PumpHasControl {
                    fan: "pump".into(),
                    field: "curve",
                },
            ),
            (
                "pump with sensor",
                |c| fan_mut(c, "pump").sensor = Some("cpu/temp1".into()),
                ValidationError::PumpHasControl {
                    fan: "pump".into(),
                    field: "sensor",
                },
            ),
            (
                "pump with min_percent",
                |c| fan_mut(c, "pump").min_percent = Some(50),
                ValidationError::PumpHasControl {
                    fan: "pump".into(),
                    field: "min_percent",
                },
            ),
            (
                "pump with smoothing",
                |c| fan_mut(c, "pump").step_up_percent = Some(5.0),
                ValidationError::PumpHasControl {
                    fan: "pump".into(),
                    field: "step_up_percent",
                },
            ),
            (
                "fan without curve",
                |c| fan_mut(c, "case").curve = None,
                ValidationError::MissingField {
                    fan: "case".into(),
                    field: "curve",
                },
            ),
            (
                "fan without sensor",
                |c| fan_mut(c, "case").sensor = None,
                ValidationError::MissingField {
                    fan: "case".into(),
                    field: "sensor",
                },
            ),
            (
                "fan without min_percent",
                |c| fan_mut(c, "case").min_percent = None,
                ValidationError::MissingField {
                    fan: "case".into(),
                    field: "min_percent",
                },
            ),
            (
                "malformed sensor",
                |c| fan_mut(c, "case").sensor = Some("k10temp-temp1".into()),
                ValidationError::InvalidSensor {
                    context: "fan 'case'".into(),
                    value: "k10temp-temp1".into(),
                },
            ),
            (
                "sensor on unknown chip",
                |c| fan_mut(c, "case").sensor = Some("gpu/temp1".into()),
                ValidationError::UnknownSensorChip {
                    context: "fan 'case'".into(),
                    value: "gpu/temp1".into(),
                    alias: "gpu".into(),
                },
            ),
            (
                "malformed cpu sensor",
                |c| c.safety.cpu_sensor = "cpu/fan1".into(),
                ValidationError::InvalidSensor {
                    context: "safety.cpu_sensor".into(),
                    value: "cpu/fan1".into(),
                },
            ),
            (
                "one-point curve",
                |c| fan_mut(c, "case").curve = Some(vec![(40.0, 50.0)]),
                ValidationError::CurveTooShort { fan: "case".into() },
            ),
            (
                "curve percent over 100",
                |c| fan_mut(c, "case").curve = Some(vec![(40.0, 50.0), (80.0, 120.0)]),
                ValidationError::CurvePointOutOfRange {
                    fan: "case".into(),
                    temp: 80.0,
                    percent: 120.0,
                },
            ),
            (
                "curve temp out of range",
                |c| fan_mut(c, "case").curve = Some(vec![(-5.0, 50.0), (80.0, 100.0)]),
                ValidationError::CurvePointOutOfRange {
                    fan: "case".into(),
                    temp: -5.0,
                    percent: 50.0,
                },
            ),
            (
                "unsorted curve",
                |c| fan_mut(c, "case").curve = Some(vec![(60.0, 50.0), (40.0, 60.0)]),
                ValidationError::CurveTempsNotIncreasing { fan: "case".into() },
            ),
            (
                "repeated curve temp",
                |c| fan_mut(c, "case").curve = Some(vec![(40.0, 50.0), (40.0, 60.0)]),
                ValidationError::CurveTempsNotIncreasing { fan: "case".into() },
            ),
            (
                "decreasing curve",
                |c| fan_mut(c, "case").curve = Some(vec![(40.0, 60.0), (60.0, 50.0)]),
                ValidationError::CurvePercentDecreasing { fan: "case".into() },
            ),
            (
                "min_percent 0",
                |c| fan_mut(c, "case").min_percent = Some(0),
                ValidationError::MinPercentOutOfRange {
                    fan: "case".into(),
                    value: 0,
                },
            ),
            (
                "min_percent over 100",
                |c| fan_mut(c, "case").min_percent = Some(101),
                ValidationError::MinPercentOutOfRange {
                    fan: "case".into(),
                    value: 101,
                },
            ),
            (
                "hysteresis too big",
                |c| fan_mut(c, "case").hysteresis_c = Some(15.0),
                ValidationError::SettingOutOfRange {
                    fan: "case".into(),
                    field: "hysteresis_c",
                    value: 15.0,
                    min: 0.0,
                    max: 10.0,
                },
            ),
            (
                "step down 0",
                |c| fan_mut(c, "case").step_down_percent = Some(0.0),
                ValidationError::SettingOutOfRange {
                    fan: "case".into(),
                    field: "step_down_percent",
                    value: 0.0,
                    min: 1.0,
                    max: 100.0,
                },
            ),
            (
                "critical too high",
                |c| c.safety.critical_temp_c = 110.0,
                ValidationError::CriticalTempOutOfRange(110.0),
            ),
            (
                "sensor fail percent 0",
                |c| c.safety.sensor_fail_percent = 0,
                ValidationError::SensorFailPercentOutOfRange(0),
            ),
            (
                "sensor fail below a fan minimum",
                |c| c.safety.sensor_fail_percent = 22,
                ValidationError::SensorFailBelowMin {
                    fan: "radiator".into(),
                    value: 22,
                    min: 25,
                },
            ),
        ];
        for (name, mutate, expected) in cases {
            let mut config = valid();
            mutate(&mut config);
            let errors = validate(&config).expect_err(name);
            assert!(
                errors.contains(&expected),
                "{name}: expected {expected:?} in {errors:?}"
            );
        }
    }

    #[test]
    fn reports_every_error_at_once() {
        let mut config = valid();
        config.version = 7;
        fan_mut(&mut config, "pump").curve = Some(vec![(30.0, 50.0), (60.0, 100.0)]);
        fan_mut(&mut config, "case").min_percent = Some(0);
        assert_eq!(validate(&config).unwrap_err().len(), 3);
    }

    #[test]
    fn warns_about_low_minimum() {
        let mut config = valid();
        assert!(warnings(&config).is_empty());
        fan_mut(&mut config, "case").min_percent = Some(15);
        assert_eq!(
            warnings(&config),
            vec![ConfigWarning::LowMinPercent {
                fan: "case".into(),
                value: 15,
            }]
        );
    }

    #[test]
    fn parses_sensor_refs() {
        let cases: &[(&str, Option<(&str, u32)>)] = &[
            ("cpu/temp1", Some(("cpu", 1))),
            ("board/temp12", Some(("board", 12))),
            ("cpu/temp0", None),
            ("cpu/temp", None),
            ("cpu/tempx", None),
            ("cpu/temp+1", None),
            ("cpu/fan1", None),
            ("/temp1", None),
            ("cpu", None),
            ("", None),
        ];
        for (input, expected) in cases {
            let got = parse_sensor_ref(input);
            let got = got.as_ref().map(|s| (s.chip_alias.as_str(), s.index));
            assert_eq!(got, *expected, "input: {input}");
        }
    }

    #[test]
    fn matches_chip_names() {
        let cases: &[(&str, &str, bool)] = &[
            ("it8689", "it8689", true),
            ("it8689", "it8689_61030507", true),
            ("it8689", "it8689_", true),
            ("it8689", "it86890", false),
            ("it8689", "it8689e", false),
            ("it8689", "it868", false),
            ("it8689", "k10temp", false),
            ("k10temp", "k10temp", true),
        ];
        for (configured, detected, expected) in cases {
            assert_eq!(
                chip_name_matches(configured, detected),
                *expected,
                "{configured} vs {detected}"
            );
        }
    }

    fn chip(dir: &str, name: &str) -> Chip {
        Chip {
            name: name.to_string(),
            path: PathBuf::from(format!("/fake/{dir}")),
            temps: vec![
                TempSensor {
                    index: 1,
                    label: None,
                    celsius: Some(40.0),
                },
                TempSensor {
                    index: 2,
                    label: None,
                    celsius: Some(40.0),
                },
            ],
            fans: Vec::new(),
            pwms: (1..=3)
                .map(|index| PwmOutput {
                    index,
                    value: Some(255),
                    mode: Some(PwmMode::Automatic(2)),
                })
                .collect(),
        }
    }

    #[test]
    fn finds_chip_by_exact_name() {
        let chips = [chip("hwmon2", "k10temp"), chip("hwmon5", "it8689")];
        let found = find_chip(&chips, "board", "it8689").unwrap();
        assert_eq!(found.path, PathBuf::from("/fake/hwmon5"));
    }

    #[test]
    fn finds_chip_by_suffixed_name() {
        let chips = [chip("hwmon2", "k10temp"), chip("hwmon5", "it8689_61030507")];
        let found = find_chip(&chips, "board", "it8689").unwrap();
        assert_eq!(found.name, "it8689_61030507");
    }

    #[test]
    fn ambiguous_chip_is_an_error() {
        let chips = [chip("hwmon4", "it8689"), chip("hwmon5", "it8689_61030507")];
        let err = find_chip(&chips, "board", "it8689").unwrap_err();
        assert_eq!(
            err,
            ResolveError::ChipAmbiguous {
                alias: "board".into(),
                name: "it8689".into(),
                matches: vec![
                    "it8689 (/fake/hwmon4)".into(),
                    "it8689_61030507 (/fake/hwmon5)".into(),
                ],
            }
        );
    }

    #[test]
    fn missing_chip_is_an_error() {
        let chips = [chip("hwmon2", "k10temp")];
        assert!(matches!(
            find_chip(&chips, "board", "it8689"),
            Err(ResolveError::ChipNotFound { .. })
        ));
    }

    #[test]
    fn resolves_only_fans() {
        let chips = [chip("hwmon2", "k10temp"), chip("hwmon5", "it8689_61030507")];
        let plan = resolve(&valid(), &chips).unwrap();
        let outputs: Vec<u32> = plan.fans.iter().map(|f| f.output.index).collect();
        assert_eq!(outputs, [1, 3]);
        assert!(!plan.controls(&OutputRef {
            chip_path: PathBuf::from("/fake/hwmon5"),
            index: 2,
        }));
        let case = &plan.fans[1];
        assert_eq!(case.original_mode, PwmMode::Automatic(2));
        assert_eq!(case.hysteresis_c, 3.0);
        assert_eq!(case.step_up_percent, 20.0);
        let radiator = &plan.fans[0];
        assert_eq!(radiator.hysteresis_c, DEFAULT_HYSTERESIS_C);
        assert_eq!(radiator.step_down_percent, DEFAULT_STEP_DOWN_PERCENT);
        assert_eq!(plan.cpu_sensor.chip_path, PathBuf::from("/fake/hwmon2"));
    }

    #[test]
    fn resolve_reports_missing_hardware() {
        let mut board = chip("hwmon5", "it8689");
        board.pwms.retain(|p| p.index != 3);
        board.pwms[0].mode = None;
        let mut cpu = chip("hwmon2", "k10temp");
        cpu.temps.clear();
        let errors = resolve(&valid(), &[cpu, board]).unwrap_err();
        assert!(errors.contains(&ResolveError::EnableUnreadable {
            fan: "radiator".into(),
            chip: "it8689".into(),
            pwm: 1,
        }));
        assert!(errors.contains(&ResolveError::PwmNotFound {
            fan: "case".into(),
            chip: "it8689".into(),
            pwm: 3,
        }));
        assert!(errors.contains(&ResolveError::SensorNotFound {
            sensor: "cpu/temp1".into(),
        }));
    }

    #[test]
    fn restore_outputs_skip_pumps() {
        let chips = [chip("hwmon2", "k10temp"), chip("hwmon5", "it8689")];
        let mut config = valid();
        // A broken config where a fan claims the pump's output.
        fan_mut(&mut config, "case").pwm = 2;
        let (outputs, errors) = restore_outputs(&config, &chips);
        assert!(errors.is_empty());
        let ids: Vec<&str> = outputs.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["radiator"]);
    }

    #[test]
    fn example_config_is_valid() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/config.example.json");
        let config = load(&path).unwrap();
        assert!(warnings(&config).is_empty());
        let pumps: Vec<u32> = config
            .fans
            .iter()
            .filter(|f| f.role == Role::Pump)
            .map(|f| f.pwm)
            .collect();
        assert_eq!(pumps, [2]);
    }

    #[test]
    fn with_curve_replaces_only_that_fan() {
        let config = valid();
        let curve = vec![(30.0, 25.0), (60.0, 50.0), (80.0, 100.0)];
        let changed = with_curve(&config, "case", curve.clone()).unwrap();
        assert_eq!(changed.fans[2].curve, Some(curve));
        assert_eq!(changed.fans[0], config.fans[0]);
        assert_eq!(changed.fans[1], config.fans[1]);
    }

    #[test]
    fn with_curve_rejects_bad_changes() {
        let config = valid();
        let good = vec![(30.0, 25.0), (80.0, 100.0)];
        let cases: Vec<(&str, Vec<CurvePoint>, ChangeError)> = vec![
            ("cpu", good.clone(), ChangeError::UnknownFan("cpu".into())),
            ("pump", good, ChangeError::Pump("pump".into())),
            (
                "case",
                vec![(60.0, 50.0), (40.0, 60.0)],
                ChangeError::Invalid(vec![ValidationError::CurveTempsNotIncreasing {
                    fan: "case".into(),
                }]),
            ),
            (
                "case",
                vec![(40.0, 50.0)],
                ChangeError::Invalid(vec![ValidationError::CurveTooShort { fan: "case".into() }]),
            ),
        ];
        for (fan, curve, expected) in cases {
            assert_eq!(with_curve(&config, fan, curve), Err(expected), "{fan}");
        }
    }

    #[test]
    fn change_errors_are_short() {
        assert_eq!(
            ChangeError::UnknownFan("cpu".into()).to_string(),
            "unknown fan id 'cpu'"
        );
        let invalid = ChangeError::Invalid(vec![
            ValidationError::CurveTooShort { fan: "case".into() },
            ValidationError::CurvePercentDecreasing { fan: "case".into() },
        ]);
        assert_eq!(
            invalid.to_string(),
            "fan 'case': curve needs at least 2 points; fan 'case': curve percentages must not decrease"
        );
    }

    #[test]
    fn serialized_config_parses_back_equal() {
        let config = valid();
        let text = serde_json::to_string_pretty(&config).unwrap();
        assert_eq!(parse(&text).unwrap(), config);
        // Optional fields that were not set stay out of the file.
        assert!(!text.contains("null"));
    }

    #[test]
    fn save_atomic_writes_a_loadable_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        let config = with_curve(&valid(), "case", vec![(30.0, 25.0), (80.0, 100.0)]).unwrap();
        save_atomic(&path, &config).unwrap();

        assert_eq!(load(&path).unwrap(), config);
        assert!(!tmp_path(&path).exists());
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn failed_save_leaves_the_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        fs::write(&path, "old").unwrap();
        // A directory in the way of the temp file makes the save fail.
        fs::create_dir(tmp_path(&path)).unwrap();
        assert!(matches!(
            save_atomic(&path, &valid()),
            Err(ConfigError::Write { .. })
        ));
        assert_eq!(fs::read_to_string(&path).unwrap(), "old");
    }

    #[test]
    fn same_outputs_compares_controlled_outputs() {
        let chips = [chip("hwmon2", "k10temp"), chip("hwmon5", "it8689")];
        let plan = resolve(&valid(), &chips).unwrap();

        let mut curve_changed = valid();
        fan_mut(&mut curve_changed, "case").curve = Some(vec![(30.0, 25.0), (80.0, 100.0)]);
        assert!(same_outputs(
            &plan,
            &resolve(&curve_changed, &chips).unwrap()
        ));

        let mut moved = valid();
        fan_mut(&mut moved, "case").pwm = 4;
        let mut board = chip("hwmon5", "it8689");
        board.pwms.push(PwmOutput {
            index: 4,
            value: Some(255),
            mode: Some(PwmMode::Automatic(2)),
        });
        let chips = [chip("hwmon2", "k10temp"), board];
        assert!(!same_outputs(&plan, &resolve(&moved, &chips).unwrap()));
    }

    #[test]
    fn config_path_is_stable() {
        assert_eq!(CONFIG_PATH, "/etc/fanarchy/config.json");
    }
}
