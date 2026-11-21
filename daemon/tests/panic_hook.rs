//! The panic hook restores `pwmN_enable` on its own, without a drop guard.
//!
//! Lives in its own test binary because the panic hook is process-wide.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::panic::catch_unwind;
use std::path::Path;
use std::sync::Arc;

use common::{FakeHwmon, RecordingHwmon};
use fanarchyd::config;
use fanarchyd::control::{Controller, Restorer, install_panic_hook};
use fanarchyd::hwmon::Hwmon;

#[test]
fn panic_hook_restores_original_modes() {
    let hw = Arc::new(RecordingHwmon::new(FakeHwmon::from_fixture(
        "it8689_k10temp",
    )));
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/config.example.json");
    let config = config::load(&path).unwrap();
    let plan = config::resolve(&config, &hw.chips().unwrap()).unwrap();

    let restorer = Arc::new(Restorer::new(Arc::clone(&hw), &plan));
    install_panic_hook(Arc::clone(&restorer));
    let _controller = Controller::start(Arc::clone(&hw), plan).unwrap();
    assert_eq!(hw.fake.read("hwmon5", "pwm1_enable"), "1");

    // No guard here: only the hook can restore.
    let result = catch_unwind(|| panic!("simulated crash"));

    assert!(result.is_err());
    assert_eq!(hw.fake.read("hwmon5", "pwm1_enable"), "2");
    assert_eq!(hw.fake.read("hwmon5", "pwm3_enable"), "2");
}
