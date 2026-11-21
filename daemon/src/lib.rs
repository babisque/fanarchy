//! Core library for `fanarchyd`.
//!
//! Data flow: sensors → curves (pure functions) → target PWM → safety
//! limits → write to `hwmon` → status to the plugin.

pub mod config;
pub mod control;
pub mod curve;
pub mod gpu;
pub mod hwmon;
pub mod ipc;
pub mod safety;
pub mod status;
