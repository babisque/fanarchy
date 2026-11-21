//! `FakeHwmon`: a fake `/sys` tree in a temp dir for tests.
//!
//! Laid out like real sysfs: chip directories live under
//! `devices/<dir>/` and `class/hwmon/<dir>` is a symlink to each one,
//! so the real `SysHwmon` read and write paths (including symlinks) are
//! exercised. `RecordingHwmon` wraps it to record every write attempt and
//! inject failures.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fanarchyd::hwmon::{Chip, Hwmon, HwmonError, OutputRef, PwmMode, SysHwmon};
use fanarchyd::safety::Duty;
use tempfile::TempDir;

pub struct FakeHwmon {
    dir: TempDir,
    sys: SysHwmon,
}

impl FakeHwmon {
    /// An empty hwmon class directory.
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("create temp dir");
        let root = dir.path().join("class/hwmon");
        fs::create_dir_all(&root).expect("create class/hwmon");
        fs::create_dir_all(dir.path().join("devices")).expect("create devices");
        Self {
            sys: SysHwmon::new(root),
            dir,
        }
    }

    /// Copy every chip directory of `tests/fixtures/<name>/` into the tree.
    pub fn from_fixture(name: &str) -> Self {
        let fake = Self::new();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        for entry in fs::read_dir(&fixture).expect("read fixture dir") {
            let entry = entry.expect("fixture entry");
            let dir_name = entry.file_name().into_string().expect("utf-8 dir name");
            let chip = fake.chip_dir(&dir_name);
            for file in fs::read_dir(entry.path()).expect("read fixture chip") {
                let file = file.expect("fixture file");
                fs::copy(file.path(), chip.path.join(file.file_name())).expect("copy fixture file");
            }
        }
        fake
    }

    /// Path of the hwmon class directory (what `SysHwmon` reads).
    pub fn root(&self) -> PathBuf {
        self.sys.root().to_path_buf()
    }

    /// Add a chip directory `dir_name` (e.g. `hwmon3`) with a `name` file.
    pub fn chip(&self, dir_name: &str, name: &str) -> FakeChip {
        let chip = self.chip_dir(dir_name);
        chip.file("name", &format!("{name}\n"));
        chip
    }

    /// Add a chip directory without any `name` file.
    pub fn chip_dir(&self, dir_name: &str) -> FakeChip {
        let real = self.dir.path().join("devices").join(dir_name);
        fs::create_dir_all(&real).expect("create chip dir");
        symlink(&real, self.root().join(dir_name)).expect("symlink chip dir");
        FakeChip { path: real }
    }

    /// Handle to a chip directory that already exists (e.g. from a fixture).
    pub fn existing(&self, dir_name: &str) -> FakeChip {
        let path = self.dir.path().join("devices").join(dir_name);
        assert!(path.is_dir(), "no chip dir {dir_name}");
        FakeChip { path }
    }

    /// Contents of a chip file, trimmed.
    pub fn read(&self, dir_name: &str, file: &str) -> String {
        let path = self.dir.path().join("devices").join(dir_name).join(file);
        fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
            .trim()
            .to_string()
    }
}

impl Hwmon for FakeHwmon {
    fn chips(&self) -> Result<Vec<Chip>, HwmonError> {
        self.sys.chips()
    }

    fn write_pwm(&self, output: &OutputRef, duty: Duty) -> Result<(), HwmonError> {
        self.sys.write_pwm(output, duty)
    }

    fn write_enable(&self, output: &OutputRef, mode: PwmMode) -> Result<(), HwmonError> {
        self.sys.write_enable(output, mode)
    }
}

/// One write attempt, identified as `"<chip dir>/<file>"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Write {
    Pwm(String, u8),
    Enable(String, u32),
}

impl Write {
    pub fn file(&self) -> &str {
        match self {
            Write::Pwm(file, _) | Write::Enable(file, _) => file,
        }
    }
}

/// Wraps a `FakeHwmon`, recording every write attempt (successful or not).
pub struct RecordingHwmon {
    pub fake: FakeHwmon,
    writes: Mutex<Vec<Write>>,
    failing: Mutex<HashSet<String>>,
    /// Mimic it87 chips that reject `pwmN` while `pwmN_enable` is not 1.
    pub reject_pwm_in_auto: bool,
}

impl RecordingHwmon {
    pub fn new(fake: FakeHwmon) -> Self {
        Self {
            fake,
            writes: Mutex::new(Vec::new()),
            failing: Mutex::new(HashSet::new()),
            reject_pwm_in_auto: false,
        }
    }

    pub fn writes(&self) -> Vec<Write> {
        self.writes.lock().unwrap().clone()
    }

    pub fn clear_writes(&self) {
        self.writes.lock().unwrap().clear();
    }

    /// Make writes to `"<chip dir>/<file>"` fail from now on.
    pub fn fail_writes_to(&self, file: &str) {
        self.failing.lock().unwrap().insert(file.to_string());
    }

    fn attempt(&self, write: Write) -> Result<(), HwmonError> {
        let file = write.file().to_string();
        self.writes.lock().unwrap().push(write);
        if self.failing.lock().unwrap().contains(&file) {
            return Err(simulated_error(&file, "simulated failure"));
        }
        Ok(())
    }
}

fn chip_dir(output: &OutputRef) -> String {
    output
        .chip_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

fn simulated_error(file: &str, message: &str) -> HwmonError {
    HwmonError::Write {
        path: PathBuf::from(file),
        source: std::io::Error::other(message.to_string()),
    }
}

impl Hwmon for RecordingHwmon {
    fn chips(&self) -> Result<Vec<Chip>, HwmonError> {
        self.fake.chips()
    }

    fn write_pwm(&self, output: &OutputRef, duty: Duty) -> Result<(), HwmonError> {
        let dir = chip_dir(output);
        let file = format!("{dir}/pwm{}", output.index);
        self.attempt(Write::Pwm(file.clone(), duty.raw()))?;
        if self.reject_pwm_in_auto
            && self.fake.read(&dir, &format!("pwm{}_enable", output.index)) != "1"
        {
            return Err(simulated_error(&file, "device busy (automatic mode)"));
        }
        self.fake.write_pwm(output, duty)
    }

    fn write_enable(&self, output: &OutputRef, mode: PwmMode) -> Result<(), HwmonError> {
        let file = format!("{}/pwm{}_enable", chip_dir(output), output.index);
        self.attempt(Write::Enable(file, mode.raw()))?;
        self.fake.write_enable(output, mode)
    }
}

/// One chip directory inside a `FakeHwmon`.
pub struct FakeChip {
    pub path: PathBuf,
}

impl FakeChip {
    /// Write an arbitrary file, newline-terminated like sysfs.
    pub fn file(&self, name: &str, contents: &str) -> &Self {
        fs::write(self.path.join(name), contents).expect("write fake file");
        self
    }

    pub fn temp(&self, index: u32, raw: &str, label: Option<&str>) -> &Self {
        self.file(&format!("temp{index}_input"), &format!("{raw}\n"));
        if let Some(label) = label {
            self.file(&format!("temp{index}_label"), &format!("{label}\n"));
        }
        self
    }

    pub fn fan(&self, index: u32, raw: &str, label: Option<&str>) -> &Self {
        self.file(&format!("fan{index}_input"), &format!("{raw}\n"));
        if let Some(label) = label {
            self.file(&format!("fan{index}_label"), &format!("{label}\n"));
        }
        self
    }

    pub fn pwm(&self, index: u32, raw: &str, enable: Option<&str>) -> &Self {
        self.file(&format!("pwm{index}"), &format!("{raw}\n"));
        if let Some(enable) = enable {
            self.file(&format!("pwm{index}_enable"), &format!("{enable}\n"));
        }
        self
    }
}
