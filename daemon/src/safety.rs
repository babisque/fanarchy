//! Safety rules: minimum floor, critical CPU limit and sensor failure.
//!
//! Pure functions only. The only way to obtain a [`Duty`] (the value
//! written to `pwmN`) is through [`clamp`], so a PWM below the fan's
//! minimum, or 0, can never reach the hardware.

use crate::curve::percent_to_pwm;

/// Lowest allowed `min_percent` in the config. Keeps every duty above 0.
pub const MIN_PERCENT_FLOOR: u8 = 1;

/// A PWM duty cycle that has passed the safety rules. Never 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duty(u8);

impl Duty {
    /// Full speed. Used as a last resort when restoring fails.
    pub const FULL: Duty = Duty(u8::MAX);

    /// Raw value for `pwmN` (1–255).
    pub fn raw(self) -> u8 {
        self.0
    }

    /// The duty as a percentage (0–100), unrounded.
    pub fn percent(self) -> f32 {
        f32::from(self.0) * 100.0 / 255.0
    }
}

/// A safety condition that overrides the curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Override {
    /// No override: the curve decides.
    #[default]
    None,
    /// CPU above the critical limit: 100 %.
    Critical,
    /// The fan's sensor or the CPU sensor cannot be read:
    /// `sensor_fail_percent`.
    SensorFailed,
}

/// Decide which override applies to one fan.
///
/// `cpu_temp_c` is the configured CPU sensor, `fan_temp_c` the sensor the
/// fan follows. `None` means the sensor could not be read. A CPU strictly
/// above `critical_temp_c` wins over everything else.
pub fn evaluate_override(
    cpu_temp_c: Option<f32>,
    fan_temp_c: Option<f32>,
    critical_temp_c: f32,
) -> Override {
    match (cpu_temp_c, fan_temp_c) {
        (Some(cpu), _) if cpu > critical_temp_c => Override::Critical,
        (None, _) | (_, None) => Override::SensorFailed,
        (Some(_), Some(_)) => Override::None,
    }
}

/// Turn a requested percentage into a safe [`Duty`].
///
/// Overrides replace the requested value, then the result is raised to
/// `min_percent` (itself at least [`MIN_PERCENT_FLOOR`]) and capped at
/// 100 %. The returned duty is never 0.
pub fn clamp(
    requested_percent: f32,
    min_percent: u8,
    over: Override,
    sensor_fail_percent: u8,
) -> Duty {
    let target = match over {
        Override::None => requested_percent,
        Override::Critical => 100.0,
        Override::SensorFailed => f32::from(sensor_fail_percent),
    };
    let floor = f32::from(min_percent.max(MIN_PERCENT_FLOOR));
    // NaN fails both comparisons; treat it as full speed.
    let percent = if target.is_nan() {
        100.0
    } else {
        target.clamp(floor, 100.0)
    };
    Duty(percent_to_pwm(percent).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_to_floor_and_overrides() {
        // (requested, min, override, sensor_fail, expected raw)
        let cases: &[(f32, u8, Override, u8, u8)] = &[
            // Curve value above the floor passes through.
            (50.0, 20, Override::None, 100, 128),
            (100.0, 20, Override::None, 100, 255),
            // Below the floor is raised to it.
            (10.0, 20, Override::None, 100, 51),
            (0.0, 20, Override::None, 100, 51),
            (-30.0, 20, Override::None, 100, 51),
            // Above 100 is capped.
            (180.0, 20, Override::None, 100, 255),
            // NaN is full speed.
            (f32::NAN, 20, Override::None, 100, 255),
            // Critical is always 100 %.
            (30.0, 20, Override::Critical, 60, 255),
            // Sensor failure uses sensor_fail_percent.
            (30.0, 20, Override::SensorFailed, 100, 255),
            (30.0, 20, Override::SensorFailed, 70, 179),
            // ...but never below the floor.
            (30.0, 80, Override::SensorFailed, 50, 204),
            // min_percent 0 is still floored at 1 %.
            (0.0, 0, Override::None, 100, 3),
        ];
        for (requested, min, over, fail, expected) in cases {
            let duty = clamp(*requested, *min, *over, *fail);
            assert_eq!(
                duty.raw(),
                *expected,
                "requested {requested} min {min} {over:?} fail {fail}"
            );
        }
    }

    #[test]
    fn duty_is_never_zero_or_below_minimum() {
        for min in 0..=100u8 {
            for requested in -10..=110 {
                let duty = clamp(requested as f32, min, Override::None, 100);
                assert!(duty.raw() > 0, "requested {requested} min {min}");
                assert!(
                    duty.raw() >= percent_to_pwm(f32::from(min)),
                    "requested {requested} min {min} gave {}",
                    duty.raw()
                );
            }
        }
    }

    #[test]
    fn evaluates_overrides() {
        // (cpu, fan sensor, critical, expected)
        let cases: &[(Option<f32>, Option<f32>, f32, Override)] = &[
            (Some(50.0), Some(45.0), 85.0, Override::None),
            // Exactly at the limit is not critical.
            (Some(85.0), Some(45.0), 85.0, Override::None),
            (Some(85.1), Some(45.0), 85.0, Override::Critical),
            (Some(95.0), Some(45.0), 85.0, Override::Critical),
            // Critical wins over a failed fan sensor.
            (Some(95.0), None, 85.0, Override::Critical),
            // Fan sensor failed.
            (Some(50.0), None, 85.0, Override::SensorFailed),
            // CPU sensor failed: every fan is affected.
            (None, Some(45.0), 85.0, Override::SensorFailed),
            (None, None, 85.0, Override::SensorFailed),
        ];
        for (cpu, fan, critical, expected) in cases {
            assert_eq!(
                evaluate_override(*cpu, *fan, *critical),
                *expected,
                "cpu {cpu:?} fan {fan:?}"
            );
        }
    }

    #[test]
    fn duty_percent_round_trips() {
        assert_eq!(Duty::FULL.percent(), 100.0);
        assert_eq!(clamp(50.0, 1, Override::None, 100).raw(), 128);
    }
}
