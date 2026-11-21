//! The fanarchy IPC protocol (v1): JSON Lines over a Unix socket.
//!
//! Every message type the daemon, `fanarchyctl` and the plugin exchange
//! is defined here and nowhere else. Documented in `docs/protocol.md`.
//!
//! Stability: once published, messages and fields are never renamed or
//! removed. New optional fields may be added.

pub mod hardware;
pub mod table;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use hardware::{Chip, Fan, GpuTemp, PwmMode, PwmOutput, TempSensor, pwm_to_percent};

/// Location of the IPC socket. Part of the public contract.
pub const SOCKET_PATH: &str = "/run/fanarchy.sock";

/// Longest request line the daemon accepts, newline included.
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// Seconds between two status messages after `subscribe`.
pub const SUBSCRIBE_INTERVAL_SECS: u64 = 2;

/// One curve point: `[temp_c, percent]`.
pub type CurvePoint = (f32, f32);

/// Every `cmd` the daemon understands, in protocol order.
pub const COMMANDS: [&str; 6] = [
    "status",
    "get_config",
    "set_curve",
    "set_mode",
    "reload",
    "subscribe",
];

/// A request line sent to the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// Current temperatures, RPM, speed and mode of each fan.
    Status,
    /// The config the daemon is running with.
    GetConfig,
    /// Replace a fan's curve. Validated, applied and saved.
    SetCurve {
        fan: String,
        points: Vec<CurvePoint>,
    },
    /// Switch a fan's mode. Not saved: resets to `curve` on restart.
    SetMode {
        fan: String,
        mode: FanMode,
        /// Required for `fixed`, not allowed otherwise.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        percent: Option<u8>,
    },
    /// Re-read the config file.
    Reload,
    /// Receive `status` now and every [`SUBSCRIBE_INTERVAL_SECS`].
    Subscribe,
}

impl Request {
    /// Parse one request line, with an error message fit for the client.
    pub fn parse(line: &str) -> Result<Request, String> {
        let value: Value =
            serde_json::from_str(line).map_err(|err| format!("invalid JSON: {err}"))?;
        let Some(cmd) = value.get("cmd") else {
            return Err("missing 'cmd'".to_string());
        };
        let Some(cmd) = cmd.as_str() else {
            return Err("'cmd' must be a string".to_string());
        };
        if !COMMANDS.contains(&cmd) {
            return Err(format!(
                "unknown cmd '{cmd}' (expected one of: {})",
                COMMANDS.join(", ")
            ));
        }
        let cmd = cmd.to_string();
        // `deny_unknown_fields` does not cover variants without fields.
        if let Some(object) = value.as_object()
            && let Some(extra) = object.keys().find(|key| {
                key.as_str() != "cmd" && !matches!(cmd.as_str(), "set_curve" | "set_mode")
            })
        {
            return Err(format!("invalid {cmd} request: unknown field `{extra}`"));
        }
        serde_json::from_value(value).map_err(|err| format!("invalid {cmd} request: {err}"))
    }
}

/// How the daemon drives a fan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FanMode {
    /// Follow the fan's curve.
    Curve,
    /// A fixed speed in %.
    Fixed,
    /// Motherboard (BIOS) control. The daemon still forces 100 % on a
    /// critical CPU or a failed sensor.
    Auto,
}

impl FanMode {
    pub fn as_str(self) -> &'static str {
        match self {
            FanMode::Curve => "curve",
            FanMode::Fixed => "fixed",
            FanMode::Auto => "auto",
        }
    }
}

/// What a `pwmN` output in the config is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Controlled by the daemon.
    Fan,
    /// Never written to, in any way.
    Pump,
}

/// A safety condition that overrides the fan's mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Override {
    /// No override: the mode decides.
    #[default]
    None,
    /// CPU above the critical limit: 100 %.
    Critical,
    /// The fan's sensor or the CPU sensor cannot be read:
    /// `sensor_fail_percent`.
    SensorFailed,
}

/// The reply to every request: `{"ok":true,"data":...}` or
/// `{"ok":false,"error":"..."}`. `data` is left out when there is none.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(data: Value) -> Self {
        Self {
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn ok_empty() -> Self {
        Self {
            ok: true,
            data: None,
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            data: None,
            error: Some(message.into()),
        }
    }

    /// One JSON line, newline included.
    pub fn to_line(&self) -> String {
        // Serializing these plain types cannot fail; fall back to a
        // hand-written error line just in case.
        let mut line = serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"ok":false,"error":"cannot encode the response"}"#.to_string());
        line.push('\n');
        line
    }
}

/// `data` of a `status` (and `subscribe`) response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// Every output in the config, pumps included, in config order.
    pub fans: Vec<FanStatus>,
    /// The configured CPU sensor (`safety.cpu_sensor`), in °C.
    pub cpu_temp_c: Option<f32>,
    /// Above this CPU temperature every controlled fan runs at 100 %.
    pub critical_temp_c: f32,
    pub gpus: Vec<GpuTemp>,
    /// Why GPUs could not be read, e.g. `nvidia-smi` failed.
    pub gpu_note: Option<String>,
    /// Every hwmon reading, as shown by `fanarchyctl status`.
    pub chips: Vec<Chip>,
}

/// One output from the config.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FanStatus {
    pub id: String,
    pub label: String,
    pub role: Role,
    /// `"<chip alias>/tempN"` the fan follows. `None` for pumps.
    pub sensor: Option<String>,
    /// Current reading of `sensor`, in °C.
    pub temp_c: Option<f32>,
    /// `fanN_input` with the same `N` as the output's `pwmN`.
    pub rpm: Option<u32>,
    /// Speed the daemon applies, in %. `None` for pumps and for fans
    /// under motherboard control (`auto` with no override).
    pub percent: Option<u8>,
    /// Lowest speed the daemon will ever set. `None` for pumps.
    pub min_percent: Option<u8>,
    /// `None` for pumps.
    pub mode: Option<FanMode>,
    /// `None` for pumps.
    #[serde(rename = "override")]
    pub safety_override: Option<Override>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn parses_spec_examples() {
        let cases = [
            (r#"{"cmd":"status"}"#, Request::Status),
            (r#"{"cmd":"get_config"}"#, Request::GetConfig),
            (
                r#"{"cmd":"set_curve","fan":"case","points":[[30,25],[60,50],[80,100]]}"#,
                Request::SetCurve {
                    fan: "case".into(),
                    points: vec![(30.0, 25.0), (60.0, 50.0), (80.0, 100.0)],
                },
            ),
            (
                r#"{"cmd":"set_mode","fan":"case","mode":"fixed","percent":60}"#,
                Request::SetMode {
                    fan: "case".into(),
                    mode: FanMode::Fixed,
                    percent: Some(60),
                },
            ),
            (
                r#"{"cmd":"set_mode","fan":"case","mode":"auto"}"#,
                Request::SetMode {
                    fan: "case".into(),
                    mode: FanMode::Auto,
                    percent: None,
                },
            ),
            (r#"{"cmd":"reload"}"#, Request::Reload),
            (r#"{"cmd":"subscribe"}"#, Request::Subscribe),
        ];
        for (line, expected) in cases {
            assert_eq!(Request::parse(line), Ok(expected), "{line}");
        }
    }

    #[test]
    fn requests_round_trip() {
        let request = Request::SetMode {
            fan: "case".into(),
            mode: FanMode::Curve,
            percent: None,
        };
        let line = serde_json::to_string(&request).unwrap();
        assert_eq!(line, r#"{"cmd":"set_mode","fan":"case","mode":"curve"}"#);
        assert_eq!(Request::parse(&line), Ok(request));
    }

    #[test]
    fn rejects_bad_requests() {
        let cases = [
            ("not json", "invalid JSON"),
            (r#"{"fan":"case"}"#, "missing 'cmd'"),
            (r#"{"cmd":1}"#, "'cmd' must be a string"),
            (r#"{"cmd":"write_sys"}"#, "unknown cmd 'write_sys'"),
            (
                r#"{"cmd":"set_curve","fan":"case"}"#,
                "invalid set_curve request: missing field `points`",
            ),
            (
                r#"{"cmd":"set_curve","fan":"case","points":[[30,25]],"path":"/sys"}"#,
                "invalid set_curve request: unknown field `path`",
            ),
            (
                r#"{"cmd":"status","verbose":true}"#,
                "invalid status request: unknown field `verbose`",
            ),
            (
                r#"{"cmd":"set_mode","fan":"case","mode":"turbo"}"#,
                "invalid set_mode request: unknown variant `turbo`",
            ),
            (
                r#"{"cmd":"set_mode","fan":"case","mode":"fixed","percent":300}"#,
                "invalid set_mode request",
            ),
        ];
        for (line, expected) in cases {
            let err = Request::parse(line).unwrap_err();
            assert!(err.starts_with(expected), "{line}: {err}");
        }
    }

    #[test]
    fn responses_match_the_spec() {
        assert_eq!(
            Response::ok(json!({"a": 1})).to_line(),
            "{\"ok\":true,\"data\":{\"a\":1}}\n"
        );
        assert_eq!(Response::ok_empty().to_line(), "{\"ok\":true}\n");
        assert_eq!(
            Response::error("unknown fan id 'cpu'").to_line(),
            "{\"ok\":false,\"error\":\"unknown fan id 'cpu'\"}\n"
        );
    }

    #[test]
    fn fan_status_uses_override_key() {
        let fan = FanStatus {
            id: "case".into(),
            label: "Case".into(),
            role: Role::Fan,
            sensor: Some("cpu/temp1".into()),
            temp_c: Some(48.5),
            rpm: Some(900),
            percent: Some(40),
            min_percent: Some(20),
            mode: Some(FanMode::Curve),
            safety_override: Some(Override::SensorFailed),
        };
        let value = serde_json::to_value(&fan).unwrap();
        assert_eq!(value["override"], "sensor_failed");
        assert_eq!(value["mode"], "curve");
        assert_eq!(value["role"], "fan");
        let back: FanStatus = serde_json::from_value(value).unwrap();
        assert_eq!(back, fan);
    }

    #[test]
    fn socket_path_is_stable() {
        assert_eq!(SOCKET_PATH, "/run/fanarchy.sock");
    }
}
