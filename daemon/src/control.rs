//! Control loop and safety rules.
//!
//! Applies curves to controlled outputs and enforces the safety rules:
//! minimum PWM floor, 100% on sensor failure or CPU above 85 °C, and
//! restoring `pwmN_enable` on shutdown. Implemented in Phase 2.
