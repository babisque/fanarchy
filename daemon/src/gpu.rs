//! NVIDIA GPU temperature through `nvidia-smi`.
//!
//! Behind the [`GpuSensor`] trait so callers can be tested without a GPU.
//! A machine without `nvidia-smi` is normal and reports no GPUs.

use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

use crate::hwmon::millidegrees_to_celsius;

/// Default program name, looked up on `PATH`.
pub const DEFAULT_NVIDIA_SMI: &str = "nvidia-smi";

const QUERY_ARGS: [&str; 2] = [
    "--query-gpu=index,name,temperature.gpu",
    "--format=csv,noheader,nounits",
];

/// Temperature of one GPU.
#[derive(Debug, Clone, PartialEq)]
pub struct GpuTemp {
    pub index: u32,
    pub name: String,
    /// `None` when the driver reports `[N/A]` or an out-of-range value.
    pub celsius: Option<f32>,
}

#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("cannot run {program}: {source}")]
    Spawn {
        program: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} failed ({status}): {stderr}")]
    CommandFailed {
        program: PathBuf,
        status: ExitStatus,
        stderr: String,
    },
    #[error("{program} printed invalid UTF-8")]
    InvalidUtf8 { program: PathBuf },
}

/// Something that can report GPU temperatures.
pub trait GpuSensor {
    /// Temperatures of every GPU found. An empty list means no GPU.
    fn read(&self) -> Result<Vec<GpuTemp>, GpuError>;
}

/// Reads GPU temperatures by running `nvidia-smi`.
#[derive(Debug, Clone)]
pub struct NvidiaSmi {
    program: PathBuf,
}

impl NvidiaSmi {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl Default for NvidiaSmi {
    fn default() -> Self {
        Self::new(DEFAULT_NVIDIA_SMI)
    }
}

impl GpuSensor for NvidiaSmi {
    fn read(&self) -> Result<Vec<GpuTemp>, GpuError> {
        let output = match Command::new(&self.program).args(QUERY_ARGS).output() {
            Ok(output) => output,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                tracing::debug!(program = %self.program.display(), "nvidia-smi not found, no NVIDIA GPU");
                return Ok(Vec::new());
            }
            Err(source) => {
                return Err(GpuError::Spawn {
                    program: self.program.clone(),
                    source,
                });
            }
        };

        if !output.status.success() {
            return Err(GpuError::CommandFailed {
                program: self.program.clone(),
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }

        let stdout = String::from_utf8(output.stdout).map_err(|_| GpuError::InvalidUtf8 {
            program: self.program.clone(),
        })?;
        Ok(parse_nvidia_smi_csv(&stdout))
    }
}

/// Parse `nvidia-smi --query-gpu=index,name,temperature.gpu
/// --format=csv,noheader,nounits` output.
///
/// The first field is the index and the last the temperature, so a GPU
/// name containing commas is kept whole. Malformed lines are skipped.
pub fn parse_nvidia_smi_csv(text: &str) -> Vec<GpuTemp> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_line(line: &str) -> Option<GpuTemp> {
    let (index, rest) = line.split_once(',')?;
    let (name, temp) = rest.rsplit_once(',')?;
    let index: u32 = index.trim().parse().ok()?;
    let name = name.trim().to_string();
    let celsius = parse_gpu_celsius(temp.trim());
    Some(GpuTemp {
        index,
        name,
        celsius,
    })
}

/// nvidia-smi prints whole degrees; reuse the hwmon range check.
fn parse_gpu_celsius(raw: &str) -> Option<f32> {
    let degrees: i64 = raw.parse().ok()?;
    millidegrees_to_celsius(&(degrees.checked_mul(1000)?).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(index: u32, name: &str, celsius: Option<f32>) -> GpuTemp {
        GpuTemp {
            index,
            name: name.to_string(),
            celsius,
        }
    }

    #[test]
    fn parses_single_gpu() {
        let out = "0, NVIDIA GeForce GTX 1660 SUPER, 46\n";
        assert_eq!(
            parse_nvidia_smi_csv(out),
            vec![gpu(0, "NVIDIA GeForce GTX 1660 SUPER", Some(46.0))]
        );
    }

    #[test]
    fn parses_multiple_gpus() {
        let out = "0, GPU A, 40\n1, GPU B, 71\n";
        assert_eq!(
            parse_nvidia_smi_csv(out),
            vec![gpu(0, "GPU A", Some(40.0)), gpu(1, "GPU B", Some(71.0))]
        );
    }

    #[test]
    fn keeps_commas_in_name() {
        let out = "0, Weird, Name, 55\n";
        assert_eq!(
            parse_nvidia_smi_csv(out),
            vec![gpu(0, "Weird, Name", Some(55.0))]
        );
    }

    #[test]
    fn not_available_temp_is_none() {
        let out = "0, GPU A, [N/A]\n";
        assert_eq!(parse_nvidia_smi_csv(out), vec![gpu(0, "GPU A", None)]);
    }

    #[test]
    fn out_of_range_temp_is_none() {
        let out = "0, GPU A, 255\n";
        assert_eq!(parse_nvidia_smi_csv(out), vec![gpu(0, "GPU A", None)]);
    }

    #[test]
    fn skips_blank_and_malformed_lines() {
        let out = "\nNo devices were found\nx, GPU, 40\n0, GPU A, 40\n";
        assert_eq!(parse_nvidia_smi_csv(out), vec![gpu(0, "GPU A", Some(40.0))]);
    }

    #[test]
    fn empty_output_is_empty() {
        assert!(parse_nvidia_smi_csv("").is_empty());
    }
}
