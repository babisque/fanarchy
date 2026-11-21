//! `NvidiaSmi` against fake `nvidia-smi` scripts.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use fanarchyd::gpu::{GpuError, GpuSensor, GpuTemp, NvidiaSmi};
use tempfile::TempDir;

/// Write an executable shell script standing in for `nvidia-smi`.
fn fake_smi(dir: &TempDir, body: &str) -> PathBuf {
    let path = dir.path().join("nvidia-smi");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    wait_until_executable(&path);
    path
}

/// Parallel tests forking while we wrote the script can briefly hold it
/// open for writing, making exec fail with ETXTBSY. Retry until it runs.
fn wait_until_executable(path: &std::path::Path) {
    const ETXTBSY: i32 = 26;
    for _ in 0..100 {
        match std::process::Command::new(path).output() {
            Err(err) if err.raw_os_error() == Some(ETXTBSY) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            _ => return,
        }
    }
    panic!("fake nvidia-smi stayed busy");
}

fn gpu(index: u32, name: &str, celsius: Option<f32>) -> GpuTemp {
    GpuTemp {
        index,
        name: name.to_string(),
        celsius,
    }
}

#[test]
fn reads_one_gpu() {
    let dir = tempfile::tempdir().unwrap();
    let smi = fake_smi(&dir, "echo '0, NVIDIA GeForce GTX 1660 SUPER, 46'");
    assert_eq!(
        NvidiaSmi::new(smi).read().unwrap(),
        vec![gpu(0, "NVIDIA GeForce GTX 1660 SUPER", Some(46.0))]
    );
}

#[test]
fn passes_query_arguments() {
    let dir = tempfile::tempdir().unwrap();
    let smi = fake_smi(
        &dir,
        r#"[ "$1" = "--query-gpu=index,name,temperature.gpu" ] && [ "$2" = "--format=csv,noheader,nounits" ] || exit 3
echo '0, GPU, 40'"#,
    );
    assert_eq!(
        NvidiaSmi::new(smi).read().unwrap(),
        vec![gpu(0, "GPU", Some(40.0))]
    );
}

#[test]
fn reads_two_gpus() {
    let dir = tempfile::tempdir().unwrap();
    let smi = fake_smi(&dir, "printf '0, GPU A, 40\\n1, GPU B, 71\\n'");
    assert_eq!(
        NvidiaSmi::new(smi).read().unwrap(),
        vec![gpu(0, "GPU A", Some(40.0)), gpu(1, "GPU B", Some(71.0))]
    );
}

#[test]
fn not_available_temp_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let smi = fake_smi(&dir, "echo '0, GPU A, [N/A]'");
    assert_eq!(
        NvidiaSmi::new(smi).read().unwrap(),
        vec![gpu(0, "GPU A", None)]
    );
}

#[test]
fn failing_command_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let smi = fake_smi(&dir, "echo 'NVIDIA-SMI has failed' >&2; exit 9");
    match NvidiaSmi::new(smi).read() {
        Err(GpuError::CommandFailed { status, stderr, .. }) => {
            assert_eq!(status.code(), Some(9));
            assert_eq!(stderr, "NVIDIA-SMI has failed");
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
}

#[test]
fn missing_binary_means_no_gpu() {
    let dir = tempfile::tempdir().unwrap();
    let smi = NvidiaSmi::new(dir.path().join("no-such-nvidia-smi"));
    assert_eq!(smi.read().unwrap(), vec![]);
}

#[test]
fn fake_sensor_through_trait() {
    struct FakeGpu(Vec<GpuTemp>);
    impl GpuSensor for FakeGpu {
        fn read(&self) -> Result<Vec<GpuTemp>, GpuError> {
            Ok(self.0.clone())
        }
    }
    let sensor: &dyn GpuSensor = &FakeGpu(vec![gpu(0, "GPU", Some(50.0))]);
    assert_eq!(sensor.read().unwrap(), vec![gpu(0, "GPU", Some(50.0))]);
}
