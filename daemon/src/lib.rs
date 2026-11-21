//! Core library for `fanarchyd`.
//!
//! Data flow: sensors → curves (pure functions) → target PWM → safety
//! limits → write to `hwmon` → status to the plugin.

pub mod config;
pub mod control;
pub mod curve;
pub mod hwmon;
pub mod ipc;
