# Spec — Phase 3: IPC and CLI

## 1. Overview and Goal
* **Purpose:** Open a channel so the plugin and the command line can talk to the daemon: query status, read the config and change curves without restarting the service.
* **Success Criteria:**
  * The daemon listens on `/run/fanarchy.sock` and accepts multiple clients at once.
  * `fanarchyctl` queries status and changes curves over the socket.
  * A curve sent over the socket is validated, applied immediately and saved to the config file.
  * An invalid request gets a clear error and changes nothing.
  * A regular (non-root) user in the `fanarchy` group can read status and change curves.

## 2. Tech Stack and Architecture
* **Transport:** Unix socket with `tokio::net::UnixListener`, permissions `0660`, owner `root:fanarchy`.
* **Format:** JSON Lines (one JSON message per line), with `serde`.
* **Messages (v1):**
  * `{"cmd":"status"}` → temperatures, RPM, current % and mode of each fan.
  * `{"cmd":"get_config"}` → current config.
  * `{"cmd":"set_curve","fan":"<id>","points":[[30,25],[60,50],[80,100]]}`
  * `{"cmd":"set_mode","fan":"<id>","mode":"curve|fixed|auto","percent":60}`
  * `{"cmd":"reload"}` → re-reads the config file.
  * `{"cmd":"subscribe"}` → receives status every 2 s without asking.
  * Response: `{"ok":true,"data":...}` or `{"ok":false,"error":"..."}`.
* **CLI:** `clap`, with subcommands `status`, `config`, `set-curve`, `set-mode` and `reload`.

## 3. Commands
* **Test:** `cargo test`
* **CLI:** `cargo run --bin fanarchyctl -- status`
* **Manual socket test:** `echo '{"cmd":"status"}' | socat - UNIX-CONNECT:/run/fanarchy.sock`

## 4. Project Structure
* `protocol/`: crate with the message types, shared by the daemon and the CLI.
* `daemon/src/ipc.rs`: socket server.
* `cli/src/main.rs`: `fanarchyctl`.
* `docs/protocol.md`: message documentation, used by the plugin.

## 5. Code Style and Conventions
* Message types live in one place (`protocol/`) and are never duplicated.
* Error messages are short and say what to do ("unknown fan id 'cpu'").
* `fanarchyctl status` uses the same table as Phase 1.

## 6. Boundaries
* **Always:**
  * Run everything arriving through the socket through the same validation and safety rules as Phase 2.
  * Save the config atomically (write to a temp file, then rename).
* **Ask first:**
  * When unsure about an implementation, propose two options and let me pick.
  * Before adding a new message to the protocol.
* **Never:**
  * NEVER rename or remove protocol messages or fields once published.
  * NEVER allow controlling a `pump` output or writing an arbitrary `/sys` path through the socket.
  * NEVER leave the socket world-writable.
