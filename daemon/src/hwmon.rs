//! Hardware access through `/sys/class/hwmon`.
//!
//! This is the only module that touches hardware. Everything else goes
//! through the [`Hwmon`] trait so it can be tested against a fake `/sys`.
//! Chips are always found by their `name` file, never by `hwmonN` index.

/// Thin abstraction over hwmon reads and writes.
///
/// Methods are added in Phase 1 (sensor reading) and Phase 2 (control).
pub trait Hwmon {}
