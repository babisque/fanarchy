//! Fan curve math: interpolation, hysteresis and smoothing.
//!
//! Pure functions only: temperature in, percent out. Safety limits
//! (minimum floor, overrides) are applied afterwards by [`crate::safety`].

/// One curve point: `(temperature in °C, fan speed in %)`, serialized as
/// a `[temp_c, percent]` pair, the same shape the IPC `set_curve` uses.
pub use fanarchy_protocol::CurvePoint;

/// Linearly interpolate the fan speed (%) for `temp_c`.
///
/// Below the first point the first percent applies, above the last point
/// the last percent. Points must be sorted by temperature (the config
/// validator guarantees it). An empty curve or a non-finite temperature
/// returns 100 % so a bad input can never slow a fan down.
pub fn evaluate(points: &[CurvePoint], temp_c: f32) -> f32 {
    let (Some(first), Some(last)) = (points.first(), points.last()) else {
        return 100.0;
    };
    if !temp_c.is_finite() {
        return 100.0;
    }
    if temp_c <= first.0 {
        return first.1;
    }
    if temp_c >= last.0 {
        return last.1;
    }
    for pair in points.windows(2) {
        let (t0, p0) = pair[0];
        let (t1, p1) = pair[1];
        if temp_c <= t1 {
            let span = t1 - t0;
            if span <= 0.0 {
                return p1;
            }
            return p0 + (p1 - p0) * (temp_c - t0) / span;
        }
    }
    last.1
}

/// Temperature hysteresis.
///
/// Returns the temperature the curve should be evaluated at (the
/// "anchor"). The anchor follows the temperature up immediately, but only
/// follows it down once it has dropped at least `hysteresis_c` below the
/// anchor. This stops a fan from hunting around a curve point.
pub fn apply_hysteresis(anchor_c: Option<f32>, temp_c: f32, hysteresis_c: f32) -> f32 {
    match anchor_c {
        None => temp_c,
        Some(anchor) if temp_c >= anchor => temp_c,
        Some(anchor) if anchor - temp_c >= hysteresis_c => temp_c,
        Some(anchor) => anchor,
    }
}

/// Limit how far the speed moves in one tick.
///
/// Rises by at most `step_up` and falls by at most `step_down` percent
/// from `previous`. With no previous value the target applies directly.
pub fn smooth(previous: Option<f32>, target: f32, step_up: f32, step_down: f32) -> f32 {
    let Some(previous) = previous else {
        return target;
    };
    let delta = target - previous;
    if delta > 0.0 {
        previous + delta.min(step_up)
    } else {
        previous - (-delta).min(step_down)
    }
}

/// Convert a percentage (0–100) to a raw PWM value (0–255), rounded.
///
/// Out-of-range values are clamped. A non-finite value maps to 255 (full
/// speed), the safe direction.
pub fn percent_to_pwm(percent: f32) -> u8 {
    if !percent.is_finite() {
        return u8::MAX;
    }
    let clamped = percent.clamp(0.0, 100.0);
    let raw = (clamped * 255.0 / 100.0).round();
    // `raw` is within 0.0..=255.0, so the cast cannot truncate.
    raw as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVE: &[CurvePoint] = &[(40.0, 30.0), (60.0, 50.0), (75.0, 80.0), (85.0, 100.0)];

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn interpolates_curve() {
        let cases: &[(&[CurvePoint], f32, f32)] = &[
            // Below the first point.
            (CURVE, 0.0, 30.0),
            (CURVE, 39.9, 30.0),
            // On points.
            (CURVE, 40.0, 30.0),
            (CURVE, 60.0, 50.0),
            (CURVE, 75.0, 80.0),
            (CURVE, 85.0, 100.0),
            // Between points.
            (CURVE, 50.0, 40.0),
            (CURVE, 67.5, 65.0),
            (CURVE, 80.0, 90.0),
            // Above the last point.
            (CURVE, 90.0, 100.0),
            (CURVE, 120.0, 100.0),
            // Two-point curve.
            (&[(30.0, 20.0), (70.0, 100.0)], 50.0, 60.0),
            // Flat segment.
            (&[(30.0, 40.0), (50.0, 40.0), (70.0, 90.0)], 45.0, 40.0),
            // Flat curve.
            (&[(0.0, 50.0), (120.0, 50.0)], 65.0, 50.0),
            // Single point.
            (&[(50.0, 35.0)], 20.0, 35.0),
            (&[(50.0, 35.0)], 80.0, 35.0),
            // Empty curve is unsafe: full speed.
            (&[], 50.0, 100.0),
        ];
        for (points, temp, expected) in cases {
            let got = evaluate(points, *temp);
            assert!(
                close(got, *expected),
                "temp {temp}: got {got}, expected {expected}"
            );
        }
    }

    #[test]
    fn non_finite_temp_is_full_speed() {
        assert_eq!(evaluate(CURVE, f32::NAN), 100.0);
        assert_eq!(evaluate(CURVE, f32::INFINITY), 100.0);
    }

    #[test]
    fn applies_hysteresis() {
        // (anchor, temp, hysteresis, expected anchor)
        let cases: &[(Option<f32>, f32, f32, f32)] = &[
            // First reading sets the anchor.
            (None, 50.0, 2.0, 50.0),
            // Rising follows immediately.
            (Some(50.0), 50.5, 2.0, 50.5),
            (Some(50.0), 70.0, 2.0, 70.0),
            // Same temperature keeps it.
            (Some(50.0), 50.0, 2.0, 50.0),
            // Falling inside the band holds the anchor.
            (Some(50.0), 49.0, 2.0, 50.0),
            (Some(50.0), 48.1, 2.0, 50.0),
            // Falling to or past the band follows.
            (Some(50.0), 48.0, 2.0, 48.0),
            (Some(50.0), 40.0, 2.0, 40.0),
            // Zero hysteresis always follows.
            (Some(50.0), 49.9, 0.0, 49.9),
        ];
        for (anchor, temp, hyst, expected) in cases {
            let got = apply_hysteresis(*anchor, *temp, *hyst);
            assert!(
                close(got, *expected),
                "anchor {anchor:?} temp {temp} hyst {hyst}: got {got}, expected {expected}"
            );
        }
    }

    #[test]
    fn smooths_steps() {
        // (previous, target, step_up, step_down, expected)
        let cases: &[(Option<f32>, f32, f32, f32, f32)] = &[
            // No previous: jump.
            (None, 80.0, 10.0, 3.0, 80.0),
            // Up limited.
            (Some(30.0), 80.0, 10.0, 3.0, 40.0),
            // Up within the step.
            (Some(30.0), 35.0, 10.0, 3.0, 35.0),
            // Down limited.
            (Some(80.0), 30.0, 10.0, 3.0, 77.0),
            // Down within the step.
            (Some(31.0), 30.0, 10.0, 3.0, 30.0),
            // Already at target.
            (Some(50.0), 50.0, 10.0, 3.0, 50.0),
            // Step of 100 means no smoothing.
            (Some(20.0), 100.0, 100.0, 100.0, 100.0),
        ];
        for (prev, target, up, down, expected) in cases {
            let got = smooth(*prev, *target, *up, *down);
            assert!(
                close(got, *expected),
                "prev {prev:?} target {target}: got {got}, expected {expected}"
            );
        }
    }

    #[test]
    fn converts_percent_to_pwm() {
        let cases: &[(f32, u8)] = &[
            (0.0, 0),
            (1.0, 3),
            (20.0, 51),
            (50.0, 128),
            (75.0, 191),
            (100.0, 255),
            (-5.0, 0),
            (150.0, 255),
            (f32::NAN, 255),
        ];
        for (input, expected) in cases {
            assert_eq!(percent_to_pwm(*input), *expected, "input: {input}");
        }
    }
}
