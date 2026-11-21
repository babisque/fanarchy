//! Config loading, validation and schema.
//!
//! An invalid config is rejected and the previous one stays active.
//! Implemented in Phase 2.

/// Location of the daemon config file. Part of the public contract.
pub const CONFIG_PATH: &str = "/etc/fanarchy/config.json";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_is_stable() {
        assert_eq!(CONFIG_PATH, "/etc/fanarchy/config.json");
    }
}
