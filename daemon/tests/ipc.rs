//! The IPC socket end to end: a real socket in a temp dir, the real
//! control loop and a fake `/sys` (fixture `it8689_k10temp`, config
//! `examples/config.example.json`: pwm1/pwm3 fans, pwm2 the pump).
//!
//! Time is paused, so control ticks and `subscribe` run instantly.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use common::{FakeHwmon, RecordingHwmon};
use fanarchyd::config;
use fanarchyd::control::{self, ControlError, PanicHook, Service};
use fanarchyd::curve;
use fanarchyd::ipc::{self, Context, IpcError, SocketFile};
use fanarchyd::status::GpuReading;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

const BOARD: &str = "hwmon5";

struct Daemon {
    dir: TempDir,
    socket: PathBuf,
    config_path: PathBuf,
    hw: Arc<RecordingHwmon>,
    stop: Option<oneshot::Sender<()>>,
    control: JoinHandle<Result<(), ControlError>>,
    server: JoinHandle<()>,
    socket_file: Option<SocketFile>,
}

fn example_text() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/config.example.json");
    fs::read_to_string(path).unwrap()
}

async fn start() -> Daemon {
    let dir = tempfile::tempdir().unwrap();
    let config_path = dir.path().join("config.json");
    fs::write(&config_path, example_text()).unwrap();
    let socket = dir.path().join("fanarchy.sock");
    // A group the test user owns, standing in for `fanarchy`.
    let gid = fs::metadata(dir.path()).unwrap().gid();
    let (listener, socket_file) = ipc::bind(&socket, Some(gid)).unwrap();

    let hw = Arc::new(RecordingHwmon::new(FakeHwmon::from_fixture(
        "it8689_k10temp",
    )));
    let (requests_tx, requests) = mpsc::channel(16);
    let (published_tx, mut published) = watch::channel(None);
    let (_gpu_tx, gpu) = watch::channel(GpuReading::default());
    let server = tokio::spawn(ipc::serve(
        listener,
        Context {
            requests: requests_tx,
            published: published.clone(),
            gpu,
        },
    ));

    let (stop, stopped) = oneshot::channel::<()>();
    let service = Service {
        config_path: config_path.clone(),
        requests,
        published: published_tx,
    };
    let config = config::load(&config_path).unwrap();
    let control = tokio::spawn(control::serve(
        Arc::clone(&hw),
        config,
        PanicHook::Skip,
        async move {
            let _ = stopped.await;
        },
        Some(service),
    ));
    published.wait_for(Option::is_some).await.unwrap();

    Daemon {
        dir,
        socket,
        config_path,
        hw,
        stop: Some(stop),
        control,
        server,
        socket_file: Some(socket_file),
    }
}

impl Daemon {
    fn board(&self, file: &str) -> String {
        self.hw.fake.read(BOARD, file)
    }

    fn config_bytes(&self) -> Vec<u8> {
        fs::read(&self.config_path).unwrap()
    }

    async fn client(&self) -> Client {
        Client::connect(&self.socket).await
    }

    /// Stop the daemon. Returns the temp dir so callers can inspect it.
    async fn shutdown(mut self) -> TempDir {
        self.stop.take().unwrap().send(()).unwrap();
        (&mut self.control).await.unwrap().unwrap();
        self.server.abort();
        drop(self.socket_file.take());
        self.dir
    }
}

struct Client {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl Client {
    async fn connect(path: &Path) -> Client {
        let (read, writer) = UnixStream::connect(path).await.unwrap().into_split();
        Client {
            reader: BufReader::new(read),
            writer,
        }
    }

    async fn send_raw(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).await.unwrap();
    }

    /// Next response line, or `None` once the daemon closed the socket.
    async fn read(&mut self) -> Option<Value> {
        let mut line = String::new();
        let read = self.reader.read_line(&mut line).await.unwrap();
        (read > 0).then(|| serde_json::from_str(&line).unwrap())
    }

    async fn call(&mut self, request: Value) -> Value {
        self.send_raw(&format!("{request}\n")).await;
        self.read().await.unwrap()
    }
}

fn error_of(response: &Value) -> &str {
    assert_eq!(response["ok"], false, "{response}");
    response["error"].as_str().unwrap()
}

fn assert_ok(response: &Value) {
    assert_eq!(response["ok"], true, "{response}");
}

async fn wait_ticks(ticks: u64) {
    tokio::time::sleep(Duration::from_secs(ticks)).await;
}

#[tokio::test(start_paused = true)]
async fn socket_is_group_only() {
    let daemon = start().await;
    let meta = fs::symlink_metadata(&daemon.socket).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o660);
    assert_eq!(meta.gid(), fs::metadata(daemon.dir.path()).unwrap().gid());
    // The private staging directory is gone.
    let names: Vec<String> = fs::read_dir(daemon.dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!names.iter().any(|n| n.contains("staging")), "{names:?}");
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn root_only_without_group() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("fanarchy.sock");
    let (_listener, _file) = ipc::bind(&socket, None).unwrap();
    let mode = fs::symlink_metadata(&socket).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[tokio::test(start_paused = true)]
async fn replaces_stale_socket_but_not_other_files() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("fanarchy.sock");
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(socket.exists(), "stale socket left behind");
    let (_listener, file) = ipc::bind(&socket, None).unwrap();
    UnixStream::connect(&socket).await.unwrap();
    drop(file);
    assert!(!socket.exists(), "socket removed on drop");

    let regular = dir.path().join("taken");
    fs::write(&regular, "keep me").unwrap();
    assert!(matches!(
        ipc::bind(&regular, None),
        Err(IpcError::NotASocket { .. })
    ));
    assert_eq!(fs::read_to_string(&regular).unwrap(), "keep me");
}

#[tokio::test(start_paused = true)]
async fn status_lists_every_output() {
    let daemon = start().await;
    let mut client = daemon.client().await;
    let response = client.call(json!({"cmd": "status"})).await;
    assert_ok(&response);
    let data = &response["data"];

    let ids: Vec<&str> = data["fans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["radiator", "pump", "case"]);
    assert_eq!(data["cpu_temp_c"], 48.25);
    assert_eq!(data["critical_temp_c"], 85.0);

    let radiator = &data["fans"][0];
    assert_eq!(radiator["mode"], "curve");
    assert_eq!(radiator["override"], "none");
    assert_eq!(radiator["percent"], 38);
    assert_eq!(radiator["min_percent"], 25);
    assert_eq!(radiator["temp_c"], 48.25);

    let pump = &data["fans"][1];
    assert_eq!(pump["role"], "pump");
    assert_eq!(pump["mode"], Value::Null);
    assert_eq!(pump["percent"], Value::Null);

    // Raw readings for the table, without any /sys paths.
    assert!(!data["chips"].as_array().unwrap().is_empty());
    assert!(!response.to_string().contains("/hwmon"));
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn set_curve_applies_and_saves() {
    let daemon = start().await;
    let mut client = daemon.client().await;
    let points = json!([[30, 60], [80, 100]]);
    let response = client
        .call(json!({"cmd": "set_curve", "fan": "case", "points": points}))
        .await;
    assert_ok(&response);

    // Saved atomically.
    let saved = config::load(&daemon.config_path).unwrap();
    let case = saved.fans.iter().find(|f| f.id == "case").unwrap();
    assert_eq!(case.curve, Some(vec![(30.0, 60.0), (80.0, 100.0)]));
    assert!(!config::tmp_path(&daemon.config_path).exists());

    // get_config reflects it.
    let response = client.call(json!({"cmd": "get_config"})).await;
    assert_eq!(
        response["data"]["fans"][2]["curve"],
        json!([[30.0, 60.0], [80.0, 100.0]])
    );

    // Applied: after smoothing, pwm3 settles on the new curve at 48.25 °C.
    wait_ticks(10).await;
    let target = curve::percent_to_pwm(curve::evaluate(&[(30.0, 60.0), (80.0, 100.0)], 48.25));
    assert_eq!(daemon.board("pwm3"), target.to_string());
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn invalid_requests_change_nothing() {
    let daemon = start().await;
    let before = daemon.config_bytes();
    let pump_before = (daemon.board("pwm2"), daemon.board("pwm2_enable"));
    let mut client = daemon.client().await;

    let cases = [
        (
            json!({"cmd": "set_curve", "fan": "case", "points": [[30, 60], [80, 40]]}),
            "fan 'case': curve percentages must not decrease",
        ),
        (
            json!({"cmd": "set_curve", "fan": "pump", "points": [[30, 60], [80, 100]]}),
            "fan 'pump' is a pump and cannot be controlled",
        ),
        (
            json!({"cmd": "set_curve", "fan": "cpu", "points": [[30, 60], [80, 100]]}),
            "unknown fan id 'cpu'",
        ),
        (
            json!({"cmd": "set_mode", "fan": "pump", "mode": "fixed", "percent": 100}),
            "fan 'pump' is a pump and cannot be controlled",
        ),
        (
            json!({"cmd": "set_mode", "fan": "case", "mode": "fixed"}),
            "percent is required for mode 'fixed'",
        ),
        (
            json!({"cmd": "set_mode", "fan": "case", "mode": "fixed", "percent": 5}),
            "fan 'case': percent 5 is out of range (20–100)",
        ),
        (
            json!({"cmd": "set_curve", "fan": "case", "points": [[30, 60], [80, 100]], "path": "/sys/class/hwmon/hwmon5/pwm2"}),
            "invalid set_curve request: unknown field `path`",
        ),
    ];
    for (request, expected) in cases {
        let response = client.call(request.clone()).await;
        assert!(
            error_of(&response).starts_with(expected),
            "{request}: {response}"
        );
    }

    wait_ticks(3).await;
    assert_eq!(daemon.config_bytes(), before);
    assert_eq!(
        (daemon.board("pwm2"), daemon.board("pwm2_enable")),
        pump_before
    );
    assert!(
        daemon
            .hw
            .writes()
            .iter()
            .all(|w| !w.file().starts_with(&format!("{BOARD}/pwm2")))
    );
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn set_mode_is_applied_but_not_saved() {
    let daemon = start().await;
    let before = daemon.config_bytes();
    let mut client = daemon.client().await;

    let response = client
        .call(json!({"cmd": "set_mode", "fan": "case", "mode": "fixed", "percent": 60}))
        .await;
    assert_ok(&response);
    wait_ticks(1).await;
    assert_eq!(daemon.board("pwm3"), "153");
    let status = client.call(json!({"cmd": "status"})).await;
    assert_eq!(status["data"]["fans"][2]["mode"], "fixed");
    assert_eq!(status["data"]["fans"][2]["percent"], 60);

    let response = client
        .call(json!({"cmd": "set_mode", "fan": "case", "mode": "auto"}))
        .await;
    assert_ok(&response);
    wait_ticks(1).await;
    assert_eq!(daemon.board("pwm3_enable"), "2");
    let status = client.call(json!({"cmd": "status"})).await;
    assert_eq!(status["data"]["fans"][2]["mode"], "auto");
    assert_eq!(status["data"]["fans"][2]["percent"], Value::Null);

    assert_ok(
        &client
            .call(json!({"cmd": "set_mode", "fan": "case", "mode": "curve"}))
            .await,
    );
    wait_ticks(1).await;
    assert_eq!(daemon.board("pwm3_enable"), "1");

    assert_eq!(daemon.config_bytes(), before);
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn reload_applies_changes_but_not_new_outputs() {
    let daemon = start().await;
    let mut client = daemon.client().await;
    let text = example_text();

    let new_curve = text.replace("[[40, 25], [65, 45], [80, 100]]", "[[30, 50], [80, 100]]");
    assert_ne!(new_curve, text);
    fs::write(&daemon.config_path, &new_curve).unwrap();
    assert_ok(&client.call(json!({"cmd": "reload"})).await);
    let config = client.call(json!({"cmd": "get_config"})).await;
    assert_eq!(
        config["data"]["fans"][2]["curve"],
        json!([[30.0, 50.0], [80.0, 100.0]])
    );

    let moved = new_curve.replace("\"pwm\": 3", "\"pwm\": 4");
    fs::write(&daemon.config_path, &moved).unwrap();
    let response = client.call(json!({"cmd": "reload"})).await;
    assert_eq!(
        error_of(&response),
        "controlled outputs changed; restart the daemon (sudo systemctl restart fanarchyd)"
    );

    fs::write(&daemon.config_path, "{ not json").unwrap();
    let response = client.call(json!({"cmd": "reload"})).await;
    assert!(error_of(&response).starts_with("cannot parse config"));

    let invalid = text.replace("\"min_percent\": 20", "\"min_percent\": 0");
    fs::write(&daemon.config_path, &invalid).unwrap();
    let response = client.call(json!({"cmd": "reload"})).await;
    assert_eq!(
        error_of(&response),
        "invalid config: fan 'case': min_percent 0 is out of range (1–100)"
    );

    // Still running the last good config.
    let config = client.call(json!({"cmd": "get_config"})).await;
    assert_eq!(config["data"]["fans"][2]["pwm"], 3);
    assert_eq!(
        config["data"]["fans"][2]["curve"],
        json!([[30.0, 50.0], [80.0, 100.0]])
    );
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn serves_several_clients_at_once() {
    let daemon = start().await;
    let mut watcher = daemon.client().await;
    watcher.send_raw("{\"cmd\":\"subscribe\"}\n").await;
    assert_ok(&watcher.read().await.unwrap());

    let mut first = daemon.client().await;
    let mut second = daemon.client().await;
    assert_ok(&first.call(json!({"cmd": "status"})).await);
    assert_ok(&second.call(json!({"cmd": "get_config"})).await);
    assert_ok(
        &second
            .call(json!({"cmd": "set_mode", "fan": "radiator", "mode": "fixed", "percent": 100}))
            .await,
    );

    // The subscriber keeps receiving and sees the change.
    let mut seen_fixed = false;
    for _ in 0..3 {
        let update = watcher.read().await.unwrap();
        assert_ok(&update);
        seen_fixed |= update["data"]["fans"][0]["mode"] == "fixed";
    }
    assert!(seen_fixed);
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn subscribe_pushes_status_every_two_seconds() {
    let daemon = start().await;
    let mut client = daemon.client().await;
    client.send_raw("{\"cmd\":\"subscribe\"}\n").await;
    let start = tokio::time::Instant::now();
    for _ in 0..3 {
        let update = client.read().await.unwrap();
        assert_ok(&update);
        assert_eq!(update["data"]["fans"].as_array().unwrap().len(), 3);
    }
    assert_eq!(start.elapsed(), Duration::from_secs(4));
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn bad_lines_get_errors_and_keep_the_connection() {
    let daemon = start().await;
    let mut client = daemon.client().await;
    client.send_raw("not json\n\n").await;
    let response = client.read().await.unwrap();
    assert!(error_of(&response).starts_with("invalid JSON"));
    let response = client.call(json!({"cmd": "fly"})).await;
    assert!(error_of(&response).starts_with("unknown cmd 'fly'"));
    assert_ok(&client.call(json!({"cmd": "status"})).await);
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn too_long_line_is_rejected_and_skipped() {
    let daemon = start().await;
    let mut client = daemon.client().await;
    let huge = format!(
        "{{\"cmd\":\"status\",\"pad\":\"{}\"}}\n",
        "x".repeat(70 * 1024)
    );
    client.send_raw(&huge).await;
    let response = client.read().await.unwrap();
    assert!(error_of(&response).starts_with("request too long"));
    // The rest of that line is thrown away; the connection still works.
    assert_ok(&client.call(json!({"cmd": "status"})).await);
    daemon.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn shutdown_restores_fans_and_removes_socket() {
    let daemon = start().await;
    let socket = daemon.socket.clone();
    let hw = Arc::clone(&daemon.hw);
    assert_eq!(daemon.board("pwm1_enable"), "1");

    let _dir = daemon.shutdown().await;
    assert!(!socket.exists());
    assert_eq!(hw.fake.read(BOARD, "pwm1_enable"), "2");
    assert_eq!(hw.fake.read(BOARD, "pwm3_enable"), "2");
    assert!(UnixStream::connect(&socket).await.is_err());
}
