//! Control loop, safety rules and restore against a fake `/sys`.
//!
//! Fixture `it8689_k10temp`: k10temp at hwmon2 (temp1 = 48.25 °C) and
//! `it8689_61030507` at hwmon5 with pwm1–pwm5, all `pwmN_enable = 2`.
//! The config is `examples/config.example.json`: pwm1 and pwm3 are fans,
//! pwm2 is the pump, pwm4/pwm5 are not in the config.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::{FakeHwmon, RecordingHwmon, Write};
use fanarchyd::config::{self, ChangeError, Config, Plan};
use fanarchyd::control::{self, ControlError, Controller, Mode, PanicHook, RestoreGuard, Restorer};
use fanarchyd::hwmon::Hwmon;

const BOARD: &str = "hwmon5";
const CPU: &str = "hwmon2";
/// Outputs the daemon must never write: the pump and unlisted outputs.
const UNTOUCHABLE: [&str; 6] = [
    "pwm2",
    "pwm2_enable",
    "pwm4",
    "pwm4_enable",
    "pwm5",
    "pwm5_enable",
];

/// 48.25 °C on the radiator curve [[40,30],[60,50],...] = 38.25 % → 98.
const RADIATOR_START: &str = "98";
/// 48.25 °C on the case curve [[40,25],[65,45],...] = 31.6 % → 81.
const CASE_START: &str = "81";

fn example_config() -> Config {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/config.example.json");
    config::load(&path).unwrap()
}

fn hardware() -> Arc<RecordingHwmon> {
    Arc::new(RecordingHwmon::new(FakeHwmon::from_fixture(
        "it8689_k10temp",
    )))
}

fn plan(hw: &RecordingHwmon, config: &Config) -> Plan {
    config::resolve(config, &hw.chips().unwrap()).unwrap()
}

fn set_cpu_temp(hw: &RecordingHwmon, raw: &str) {
    hw.fake.existing(CPU).temp(1, raw, None);
}

fn board(hw: &RecordingHwmon, file: &str) -> String {
    hw.fake.read(BOARD, file)
}

fn assert_untouched(hw: &RecordingHwmon, original: &[(&str, String)]) {
    for write in hw.writes() {
        for file in UNTOUCHABLE {
            assert_ne!(write.file(), format!("{BOARD}/{file}"), "wrote {write:?}");
        }
    }
    for (file, before) in original {
        assert_eq!(&board(hw, file), before, "{file} changed");
    }
}

fn snapshot_untouchable(hw: &RecordingHwmon) -> Vec<(&'static str, String)> {
    UNTOUCHABLE.iter().map(|f| (*f, board(hw, f))).collect()
}

#[test]
fn takes_control_with_safe_pwm_first() {
    let hw = hardware();
    Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();

    let pwm1: Vec<Write> = hw
        .writes()
        .into_iter()
        .filter(|w| w.file().starts_with(&format!("{BOARD}/pwm1")))
        .collect();
    assert_eq!(
        pwm1,
        [
            Write::Pwm(format!("{BOARD}/pwm1"), 98),
            Write::Enable(format!("{BOARD}/pwm1_enable"), 1),
            Write::Pwm(format!("{BOARD}/pwm1"), 98),
        ]
    );
    assert_eq!(board(&hw, "pwm1"), RADIATOR_START);
    assert_eq!(board(&hw, "pwm1_enable"), "1");
    assert_eq!(board(&hw, "pwm3"), CASE_START);
    assert_eq!(board(&hw, "pwm3_enable"), "1");
}

#[test]
fn ebusy_pre_write_in_automatic_mode_is_tolerated() {
    // Real it87 behaviour: pwmN writes fail with EBUSY while enable = 2.
    let mut recording = RecordingHwmon::new(FakeHwmon::from_fixture("it8689_k10temp"));
    recording.reject_pwm_in_auto = true;
    let hw = Arc::new(recording);

    Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();

    // Pre-write attempted (and rejected), then manual, then the duty.
    let pwm1: Vec<Write> = hw
        .writes()
        .into_iter()
        .filter(|w| w.file().starts_with(&format!("{BOARD}/pwm1")))
        .collect();
    assert_eq!(
        pwm1,
        [
            Write::Pwm(format!("{BOARD}/pwm1"), 98),
            Write::Enable(format!("{BOARD}/pwm1_enable"), 1),
            Write::Pwm(format!("{BOARD}/pwm1"), 98),
        ]
    );
    assert_eq!(board(&hw, "pwm1"), RADIATOR_START);
    assert_eq!(board(&hw, "pwm1_enable"), "1");
    assert_eq!(board(&hw, "pwm3"), CASE_START);
    assert_eq!(board(&hw, "pwm3_enable"), "1");
}

#[test]
fn never_touches_pump_or_unlisted_outputs() {
    let hw = hardware();
    let original = snapshot_untouchable(&hw);
    let config = example_config();
    let plan = plan(&hw, &config);
    let restorer = Restorer::new(Arc::clone(&hw), &plan);
    let mut controller = Controller::start(Arc::clone(&hw), plan).unwrap();

    for raw in ["30000", "70000", "90000", "-127000", "50000"] {
        set_cpu_temp(&hw, raw);
        controller.tick().unwrap();
    }
    assert!(restorer.restore());
    control::restore_from_config(hw.as_ref(), &config).unwrap();

    assert_untouched(&hw, &original);
}

#[test]
fn writes_only_when_duty_changes() {
    let hw = hardware();
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();
    hw.clear_writes();

    for _ in 0..5 {
        controller.tick().unwrap();
    }
    assert!(
        hw.writes().is_empty(),
        "unexpected writes: {:?}",
        hw.writes()
    );

    set_cpu_temp(&hw, "70000");
    controller.tick().unwrap();
    assert_eq!(hw.writes().len(), 2, "{:?}", hw.writes());
}

#[test]
fn reacts_to_temperature_on_next_tick() {
    let hw = hardware();
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();
    set_cpu_temp(&hw, "70000");
    controller.tick().unwrap();
    let pwm1: u8 = board(&hw, "pwm1").parse().unwrap();
    assert!(pwm1 > 98, "pwm1 = {pwm1}");
}

#[test]
fn critical_cpu_drives_every_fan_to_full() {
    let hw = hardware();
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();
    set_cpu_temp(&hw, "85500");
    controller.tick().unwrap();
    assert_eq!(board(&hw, "pwm1"), "255");
    assert_eq!(board(&hw, "pwm3"), "255");
}

#[test]
fn failed_fan_sensor_drives_only_that_fan_to_full() {
    let hw = hardware();
    let mut config = example_config();
    // Case fans follow the board sensor (44 °C); the CPU stays readable.
    let case = config.fans.iter_mut().find(|f| f.id == "case").unwrap();
    case.sensor = Some("board/temp2".into());
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &config)).unwrap();
    let pwm3_before = board(&hw, "pwm3");

    // Disconnected thermistor.
    hw.fake.existing(BOARD).temp(2, "-128000", None);
    controller.tick().unwrap();
    assert_eq!(board(&hw, "pwm3"), "255");
    assert_eq!(board(&hw, "pwm1"), RADIATOR_START);
    assert_ne!(pwm3_before, "255");
}

#[test]
fn missing_sensor_file_counts_as_failure() {
    let hw = hardware();
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();
    std::fs::remove_file(hw.fake.existing(CPU).path.join("temp1_input")).unwrap();
    controller.tick().unwrap();
    assert_eq!(board(&hw, "pwm1"), "255");
    assert_eq!(board(&hw, "pwm3"), "255");
}

#[test]
fn never_writes_below_minimum() {
    let hw = hardware();
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();
    // 0 °C: radiator curve says 30 % but min is 25; case says 25 %, min 20.
    set_cpu_temp(&hw, "0");
    for _ in 0..40 {
        controller.tick().unwrap();
        let pwm1: u8 = board(&hw, "pwm1").parse().unwrap();
        let pwm3: u8 = board(&hw, "pwm3").parse().unwrap();
        assert!(pwm1 >= 64, "pwm1 = {pwm1}");
        assert!(pwm3 >= 51, "pwm3 = {pwm3}");
    }
}

#[test]
fn repeated_write_failures_stop_the_controller() {
    let hw = hardware();
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();
    hw.fail_writes_to(&format!("{BOARD}/pwm1"));
    set_cpu_temp(&hw, "70000");
    for _ in 1..control::MAX_FAILED_TICKS {
        controller.tick().unwrap();
    }
    assert!(matches!(
        controller.tick(),
        Err(ControlError::TooManyFailures(_))
    ));
}

#[test]
fn restore_runs_once() {
    let hw = hardware();
    let plan = plan(&hw, &example_config());
    let restorer = Restorer::new(Arc::clone(&hw), &plan);
    Controller::start(Arc::clone(&hw), plan).unwrap();
    hw.clear_writes();

    assert!(restorer.restore());
    assert!(restorer.restore());
    assert_eq!(
        hw.writes(),
        [
            Write::Enable(format!("{BOARD}/pwm1_enable"), 2),
            Write::Enable(format!("{BOARD}/pwm3_enable"), 2),
        ]
    );
}

#[test]
fn original_manual_mode_is_restored_as_automatic() {
    let hw = hardware();
    hw.fake.existing(BOARD).pwm(1, "200", Some("1"));
    let plan = plan(&hw, &example_config());
    let restorer = Restorer::new(Arc::clone(&hw), &plan);
    Controller::start(Arc::clone(&hw), plan).unwrap();
    restorer.restore();
    assert_eq!(board(&hw, "pwm1_enable"), "2");
}

#[test]
fn restores_on_panic_through_drop_guard() {
    let hw = hardware();
    let plan = plan(&hw, &example_config());
    let restorer = Arc::new(Restorer::new(Arc::clone(&hw), &plan));

    let result = catch_unwind(AssertUnwindSafe(|| {
        let _guard = RestoreGuard(Arc::clone(&restorer));
        let _controller = Controller::start(Arc::clone(&hw), plan).unwrap();
        assert_eq!(board(&hw, "pwm1_enable"), "1");
        panic!("simulated crash");
    }));

    assert!(result.is_err());
    assert_eq!(board(&hw, "pwm1_enable"), "2");
    assert_eq!(board(&hw, "pwm3_enable"), "2");
}

#[tokio::test]
async fn failed_takeover_restores_and_falls_back_to_full_speed() {
    let hw = hardware();
    hw.fail_writes_to(&format!("{BOARD}/pwm3_enable"));

    let result = control::run(
        Arc::clone(&hw),
        &example_config(),
        PanicHook::Skip,
        std::future::pending(),
    )
    .await;

    assert!(
        matches!(result, Err(ControlError::Takeover { ref fan, .. }) if fan == "case"),
        "{result:?}"
    );
    // pwm1 was taken over and is back to automatic.
    assert_eq!(board(&hw, "pwm1_enable"), "2");
    // pwm3_enable cannot be written, so pwm3 is driven to full speed.
    assert_eq!(board(&hw, "pwm3"), "255");
}

#[tokio::test]
async fn ambiguous_chip_refuses_to_start() {
    let hw = hardware();
    hw.fake.chip("hwmon7", "it8689").pwm(1, "255", Some("2"));
    let result = control::run(
        Arc::clone(&hw),
        &example_config(),
        PanicHook::Skip,
        std::future::pending(),
    )
    .await;
    assert!(
        matches!(result, Err(ControlError::Resolve(_))),
        "{result:?}"
    );
    assert!(hw.writes().is_empty());
}

#[tokio::test(start_paused = true)]
async fn loop_ticks_every_second_and_restores_on_shutdown() {
    let hw = hardware();
    let config = example_config();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn({
        let hw = Arc::clone(&hw);
        async move {
            let shutdown = async {
                let _ = stopped.await;
            };
            control::run(hw, &config, PanicHook::Skip, shutdown).await
        }
    });

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(board(&hw, "pwm1"), RADIATOR_START);
    assert_eq!(board(&hw, "pwm1_enable"), "1");

    // A temperature change shows up within 2 s.
    set_cpu_temp(&hw, "70000");
    tokio::time::sleep(Duration::from_secs(2)).await;
    let pwm1: u8 = board(&hw, "pwm1").parse().unwrap();
    assert!(pwm1 > 98, "pwm1 = {pwm1}");

    stop.send(()).unwrap();
    task.await.unwrap().unwrap();
    assert_eq!(board(&hw, "pwm1_enable"), "2");
    assert_eq!(board(&hw, "pwm3_enable"), "2");
}

#[test]
fn restore_command_resets_fans_and_skips_pump() {
    let hw = hardware();
    // State left behind by a SIGKILLed daemon (pump set by something else).
    for index in 1..=3 {
        hw.fake.existing(BOARD).pwm(index, "60", Some("1"));
    }

    let report = control::restore_from_config(hw.as_ref(), &example_config()).unwrap();

    assert!(report.is_ok());
    assert_eq!(report.restored, ["radiator", "case"]);
    assert_eq!(board(&hw, "pwm1_enable"), "2");
    assert_eq!(board(&hw, "pwm3_enable"), "2");
    assert_eq!(board(&hw, "pwm2_enable"), "1");
    assert_eq!(
        hw.writes(),
        [
            Write::Enable(format!("{BOARD}/pwm1_enable"), 2),
            Write::Enable(format!("{BOARD}/pwm3_enable"), 2),
        ]
    );
}

#[test]
fn restore_command_reports_failures() {
    let hw = hardware();
    hw.fail_writes_to(&format!("{BOARD}/pwm1_enable"));
    let report = control::restore_from_config(hw.as_ref(), &example_config()).unwrap();
    assert!(!report.is_ok());
    assert_eq!(report.failed, ["radiator"]);
    assert_eq!(board(&hw, "pwm1"), "255");
    assert_eq!(board(&hw, "pwm3_enable"), "2");
}

#[test]
fn fixed_mode_sets_exact_speed() {
    let hw = hardware();
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();
    controller.set_mode("case", Mode::Fixed(60)).unwrap();
    controller.tick().unwrap();
    // 60 % → 153, no smoothing.
    assert_eq!(board(&hw, "pwm3"), "153");
    assert_eq!(board(&hw, "pwm3_enable"), "1");
    // The other fan keeps following its curve.
    assert_eq!(board(&hw, "pwm1"), RADIATOR_START);
}

#[test]
fn auto_mode_hands_fan_to_bios_but_keeps_safety() {
    let hw = hardware();
    let original = snapshot_untouchable(&hw);
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();
    hw.clear_writes();

    controller.set_mode("case", Mode::Auto).unwrap();
    controller.tick().unwrap();
    controller.tick().unwrap();
    // Exactly one write: pwm3_enable back to automatic.
    assert_eq!(
        hw.writes(),
        [Write::Enable(format!("{BOARD}/pwm3_enable"), 2)]
    );
    let snapshot = controller.snapshot();
    let case = snapshot.fans.iter().find(|f| f.id == "case").unwrap();
    assert_eq!((case.mode, case.percent), (Mode::Auto, None));

    // Critical CPU: the daemon takes the fan back at full speed.
    hw.clear_writes();
    set_cpu_temp(&hw, "90000");
    controller.tick().unwrap();
    assert_eq!(board(&hw, "pwm3"), "255");
    assert_eq!(board(&hw, "pwm3_enable"), "1");
    assert!(
        hw.writes()
            .contains(&Write::Enable(format!("{BOARD}/pwm3_enable"), 1))
    );

    // Cleared: back to the motherboard.
    set_cpu_temp(&hw, "50000");
    controller.tick().unwrap();
    assert_eq!(board(&hw, "pwm3_enable"), "2");

    // Back to curve mode: taken over again.
    controller.set_mode("case", Mode::Curve).unwrap();
    controller.tick().unwrap();
    assert_eq!(board(&hw, "pwm3_enable"), "1");

    assert_untouched(&hw, &original);
}

#[test]
fn auto_mode_with_driver_rejecting_pwm_in_auto() {
    // Taking the fan back must not get stuck on the it87 EBUSY behaviour.
    let mut recording = RecordingHwmon::new(FakeHwmon::from_fixture("it8689_k10temp"));
    recording.reject_pwm_in_auto = true;
    let hw = Arc::new(recording);
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();

    controller.set_mode("case", Mode::Auto).unwrap();
    controller.tick().unwrap();
    assert_eq!(board(&hw, "pwm3_enable"), "2");
    set_cpu_temp(&hw, "90000");
    controller.tick().unwrap();
    assert_eq!(board(&hw, "pwm3"), "255");
    assert_eq!(board(&hw, "pwm3_enable"), "1");
}

#[test]
fn unknown_or_pump_ids_cannot_be_changed() {
    let hw = hardware();
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &example_config())).unwrap();
    for id in ["pump", "cpu"] {
        assert_eq!(
            controller.set_mode(id, Mode::Fixed(100)),
            Err(ChangeError::UnknownFan(id.into()))
        );
        assert!(
            controller
                .set_curve(id, vec![(30.0, 100.0), (40.0, 100.0)])
                .is_err()
        );
    }
}

#[test]
fn replace_plan_keeps_outputs_and_resets_modes() {
    let hw = hardware();
    let config = example_config();
    let mut controller = Controller::start(Arc::clone(&hw), plan(&hw, &config)).unwrap();
    controller.set_mode("case", Mode::Auto).unwrap();
    controller.tick().unwrap();
    assert_eq!(board(&hw, "pwm3_enable"), "2");

    let mut changed = config.clone();
    changed.fans[2].curve = Some(vec![(30.0, 100.0), (80.0, 100.0)]);
    let new_plan = controller.resolve(&changed).unwrap();
    assert!(config::same_outputs(controller.plan(), &new_plan));
    controller.replace_plan(new_plan);
    controller.tick().unwrap();

    // Curve mode again, taken back over, heading for the new 100 % curve.
    assert_eq!(board(&hw, "pwm3_enable"), "1");
    let snapshot = controller.snapshot();
    let case = snapshot.fans.iter().find(|f| f.id == "case").unwrap();
    assert_eq!(case.mode, Mode::Curve);
}
