//! `SysHwmon` against fake `/sys` trees.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;

use common::FakeHwmon;
use fanarchyd::hwmon::{Chip, Fan, Hwmon, HwmonError, PwmMode, PwmOutput, SysHwmon, TempSensor};

fn only_chip(fake: &FakeHwmon) -> Chip {
    let mut chips = fake.chips().unwrap();
    assert_eq!(chips.len(), 1, "expected exactly one chip: {chips:?}");
    chips.remove(0)
}

fn temp(index: u32, label: Option<&str>, celsius: Option<f32>) -> TempSensor {
    TempSensor {
        index,
        label: label.map(str::to_string),
        celsius,
    }
}

fn fan(index: u32, rpm: Option<u32>) -> Fan {
    Fan {
        index,
        label: None,
        rpm,
    }
}

fn pwm(index: u32, value: Option<u8>, mode: Option<PwmMode>) -> PwmOutput {
    PwmOutput { index, value, mode }
}

#[test]
fn finds_chips_by_name_not_index() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon9", "k10temp").temp(1, "40000", None);
    fake.chip("hwmon0", "zzz").temp(1, "30000", None);
    fake.chip("hwmon4", "it8689").temp(1, "35000", None);

    let names: Vec<String> = fake.chips().unwrap().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["it8689", "k10temp", "zzz"]);
}

#[test]
fn chip_path_follows_symlinked_dir() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon3", "k10temp");
    let chip = only_chip(&fake);
    assert_eq!(chip.path, fake.root().join("hwmon3"));
}

#[test]
fn reads_temps_with_and_without_labels() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon1", "k10temp")
        .temp(3, "52500", Some("Tccd1"))
        .temp(1, "48250", Some("Tctl"))
        .temp(2, "41000", None);

    assert_eq!(
        only_chip(&fake).temps,
        vec![
            temp(1, Some("Tctl"), Some(48.25)),
            temp(2, None, Some(41.0)),
            temp(3, Some("Tccd1"), Some(52.5)),
        ]
    );
}

#[test]
fn out_of_range_temps_are_unavailable() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon1", "it8689")
        .temp(1, "-127000", None)
        .temp(2, "127000", None)
        .temp(3, "-128000", None)
        .temp(4, "garbage", None);

    let temps = only_chip(&fake).temps;
    assert_eq!(temps.len(), 4);
    assert!(temps.iter().all(|t| t.celsius.is_none()), "{temps:?}");
}

#[test]
fn reads_fans_running_stopped_and_garbage() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon1", "it8689")
        .fan(1, "1477", Some("CPU"))
        .fan(2, "0", None)
        .fan(3, "65535", None)
        .fan(4, "", None);

    let fans = only_chip(&fake).fans;
    assert_eq!(
        fans,
        vec![
            Fan {
                index: 1,
                label: Some("CPU".to_string()),
                rpm: Some(1477)
            },
            fan(2, Some(0)),
            fan(3, None),
            fan(4, None),
        ]
    );
}

#[test]
fn reads_pwm_with_and_without_enable() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon1", "it8689")
        .pwm(1, "255", Some("2"))
        .pwm(2, "128", Some("1"))
        .pwm(3, "0", Some("0"))
        .pwm(4, "77", None)
        .pwm(5, "300", Some("x"));

    assert_eq!(
        only_chip(&fake).pwms,
        vec![
            pwm(1, Some(255), Some(PwmMode::Automatic(2))),
            pwm(2, Some(128), Some(PwmMode::Manual)),
            pwm(3, Some(0), Some(PwmMode::Disabled)),
            pwm(4, Some(77), None),
            pwm(5, None, None),
        ]
    );
}

#[test]
fn enable_without_pwm_is_not_an_output() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon1", "it8689").file("pwm1_enable", "2\n");
    assert!(only_chip(&fake).pwms.is_empty());
}

#[test]
fn labels_without_inputs_are_ignored() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon1", "it8689")
        .file("temp7_label", "orphan\n")
        .file("fan7_label", "orphan\n");
    let chip = only_chip(&fake);
    assert!(chip.temps.is_empty() && chip.fans.is_empty());
}

#[test]
fn ignores_unrelated_files() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon1", "it8689")
        .file("in0_input", "1032\n")
        .file("temp1_max", "127000\n")
        .file("fan1_min", "0\n")
        .file("pwm1_auto_point1_temp", "40000\n")
        .file("pwm1_freq", "23437\n");

    let chip = only_chip(&fake);
    assert!(
        chip.temps.is_empty() && chip.fans.is_empty() && chip.pwms.is_empty(),
        "{chip:?}"
    );
}

#[test]
fn empty_label_is_none() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon1", "k10temp").temp(1, "40000", Some(""));
    assert_eq!(only_chip(&fake).temps, vec![temp(1, None, Some(40.0))]);
}

#[test]
fn unreadable_file_is_unavailable() {
    let fake = FakeHwmon::new();
    let chip = fake.chip("hwmon1", "k10temp");
    chip.temp(1, "40000", None);
    // A directory where a file is expected cannot be read as text,
    // even as root (unlike a chmod 000 file).
    fs::create_dir(chip.path.join("temp2_input")).unwrap();

    assert_eq!(
        only_chip(&fake).temps,
        vec![temp(1, None, Some(40.0)), temp(2, None, None)]
    );
}

#[test]
fn unreadable_permissions_are_unavailable() {
    let fake = FakeHwmon::new();
    let chip = fake.chip("hwmon1", "k10temp");
    chip.temp(1, "40000", None);
    let path = chip.path.join("temp1_input");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    // Root ignores file permissions; only assert when the read really fails.
    if fs::read_to_string(&path).is_ok() {
        return;
    }
    assert_eq!(only_chip(&fake).temps, vec![temp(1, None, None)]);
}

#[test]
fn chip_without_name_is_skipped() {
    let fake = FakeHwmon::new();
    fake.chip_dir("hwmon0").temp(1, "40000", None);
    fake.chip("hwmon1", "k10temp");
    fake.chip("hwmon2", "  \n");

    let names: Vec<String> = fake.chips().unwrap().into_iter().map(|c| c.name).collect();
    assert_eq!(names, ["k10temp"]);
}

#[test]
fn stray_files_in_root_are_ignored() {
    let fake = FakeHwmon::new();
    fs::write(fake.root().join("uevent"), "x\n").unwrap();
    fake.chip("hwmon0", "k10temp");
    assert_eq!(fake.chips().unwrap().len(), 1);
}

#[test]
fn empty_root_has_no_chips() {
    let fake = FakeHwmon::new();
    assert!(fake.chips().unwrap().is_empty());
}

#[test]
fn missing_root_is_an_error() {
    let fake = FakeHwmon::new();
    let sys = SysHwmon::new(fake.root().join("does-not-exist"));
    assert!(matches!(
        sys.chips(),
        Err(HwmonError::RootUnreadable { .. })
    ));
}

#[test]
fn reads_it8689_k10temp_fixture() {
    let fake = FakeHwmon::from_fixture("it8689_k10temp");
    let chips = fake.chips().unwrap();
    let names: Vec<&str> = chips.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["acpitz", "it8689_61030507", "k10temp"]);

    let it = &chips[1];
    assert_eq!(
        it.temps,
        vec![
            temp(1, None, Some(34.0)),
            temp(2, None, Some(44.0)),
            temp(3, None, Some(48.0)),
            temp(4, None, Some(25.0)),
            temp(5, None, Some(43.0)),
            temp(6, None, Some(51.0)),
        ]
    );
    assert_eq!(
        it.fans,
        vec![fan(1, Some(1477)), fan(2, Some(1562)), fan(3, Some(815))]
    );
    let auto = Some(PwmMode::Automatic(2));
    assert_eq!(
        it.pwms,
        vec![
            pwm(1, Some(255), auto),
            pwm(2, Some(255), auto),
            pwm(3, Some(255), auto),
            pwm(4, Some(0), auto),
            pwm(5, Some(0), auto),
        ]
    );

    let k10 = &chips[2];
    assert_eq!(
        k10.temps,
        vec![
            temp(1, Some("Tctl"), Some(48.25)),
            temp(3, Some("Tccd1"), Some(52.5))
        ]
    );
    assert!(k10.fans.is_empty() && k10.pwms.is_empty());
}

#[test]
fn reading_does_not_modify_the_tree() {
    let fake = FakeHwmon::from_fixture("it8689_k10temp");
    let before = snapshot(&fake.root());
    fake.chips().unwrap();
    assert_eq!(snapshot(&fake.root()), before);
}

/// Every file under the class dir (following symlinks) with its contents.
fn snapshot(root: &std::path::Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for chip in fs::read_dir(root).unwrap() {
        let chip = chip.unwrap().path();
        for file in fs::read_dir(&chip).unwrap() {
            let file = file.unwrap().path();
            out.push((
                file.display().to_string(),
                fs::read_to_string(&file).unwrap(),
            ));
        }
    }
    out.sort();
    out
}

fn output(fake: &FakeHwmon, dir: &str, index: u32) -> fanarchyd::hwmon::OutputRef {
    fanarchyd::hwmon::OutputRef {
        chip_path: fake.root().join(dir),
        index,
    }
}

/// A valid duty obtained the only way possible: through the safety clamp.
fn duty(percent: f32) -> fanarchyd::safety::Duty {
    fanarchyd::safety::clamp(percent, 1, fanarchyd::safety::Override::None, 100)
}

#[test]
fn writes_pwm_and_enable_through_symlink() {
    let fake = FakeHwmon::new();
    fake.chip("hwmon5", "it8689").pwm(1, "255", Some("2"));
    let out = output(&fake, "hwmon5", 1);

    fake.write_enable(&out, PwmMode::Manual).unwrap();
    fake.write_pwm(&out, duty(50.0)).unwrap();

    assert_eq!(fake.read("hwmon5", "pwm1_enable"), "1");
    assert_eq!(fake.read("hwmon5", "pwm1"), "128");
    assert_eq!(
        only_chip(&fake).pwms,
        [pwm(1, Some(128), Some(PwmMode::Manual))]
    );
}

#[test]
fn write_never_creates_missing_files() {
    let fake = FakeHwmon::new();
    let chip = fake.chip("hwmon5", "it8689");
    chip.pwm(1, "255", None);
    let out = output(&fake, "hwmon5", 1);

    let err = fake.write_enable(&out, PwmMode::Manual).unwrap_err();
    assert!(matches!(err, HwmonError::Write { .. }), "{err:?}");
    assert!(!chip.path.join("pwm1_enable").exists());

    let missing = output(&fake, "hwmon5", 9);
    assert!(fake.write_pwm(&missing, duty(50.0)).is_err());
    assert!(!chip.path.join("pwm9").exists());
}
