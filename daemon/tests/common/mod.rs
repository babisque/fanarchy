//! `FakeHwmon`: a fake `/sys` tree in a temp dir for tests.
//!
//! Laid out like real sysfs: chip directories live under
//! `devices/<dir>/` and `class/hwmon/<dir>` is a symlink to each one,
//! so the real `SysHwmon` read path (including symlinks) is exercised.

#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use fanarchyd::hwmon::{Chip, Hwmon, HwmonError, SysHwmon};
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
}

impl Hwmon for FakeHwmon {
    fn chips(&self) -> Result<Vec<Chip>, HwmonError> {
        self.sys.chips()
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
