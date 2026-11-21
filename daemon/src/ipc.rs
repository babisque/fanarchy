//! Unix socket server speaking JSON Lines.
//!
//! Messages: `status`, `get_config`, `set_curve`, `set_mode`, `reload`,
//! `subscribe`. Implemented in Phase 3.

/// Location of the IPC socket. Part of the public contract.
pub const SOCKET_PATH: &str = "/run/fanarchy.sock";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_path_is_stable() {
        assert_eq!(SOCKET_PATH, "/run/fanarchy.sock");
    }
}
