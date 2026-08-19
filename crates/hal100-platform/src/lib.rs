mod keychain;
mod sidecar_launch;

use std::{path::Path, process::Command};

use hal100_core::SystemProbe;
use hal100_protocol::{HardwareProfile, HardwareRecommendation, PlatformSummary};
use thiserror::Error;

pub use keychain::{DEFAULT_KEYCHAIN_SERVICE, MacOsKeychainSecretStore};
pub use sidecar_launch::{
    AgentKernelLaunchSpec, SidecarIsolation, SidecarLaunchError, prepare_agent_kernel_command,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct MacOsSystemProbe;

#[derive(Debug, Error)]
pub enum HardwareProbeError {
    #[error("当前构建不支持 Apple Silicon 硬件检测")]
    UnsupportedPlatform,
    #[error("无法读取系统字段 {field}：{message}")]
    SystemField {
        field: &'static str,
        message: String,
    },
    #[error("系统字段 {field} 返回了无效数值")]
    InvalidNumber { field: &'static str },
    #[error("无法读取模型目录可用空间：{0}")]
    Storage(String),
}

impl SystemProbe for MacOsSystemProbe {
    fn platform_summary(&self) -> PlatformSummary {
        PlatformSummary {
            os: "macOS".to_owned(),
            architecture: if cfg!(target_arch = "aarch64") {
                "Apple Silicon".to_owned()
            } else {
                std::env::consts::ARCH.to_owned()
            },
            supported: cfg!(all(target_os = "macos", target_arch = "aarch64")),
        }
    }
}

impl MacOsSystemProbe {
    pub fn model_storage_available_bytes(
        &self,
        model_storage_path: &Path,
    ) -> Result<u64, HardwareProbeError> {
        if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Err(HardwareProbeError::UnsupportedPlatform);
        }
        available_storage_bytes(model_storage_path)
    }

    pub fn hardware_profile(
        &self,
        model_storage_path: &Path,
    ) -> Result<HardwareProfile, HardwareProbeError> {
        if !cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            return Err(HardwareProbeError::UnsupportedPlatform);
        }

        let values = sysctl_values(&[
            "hw.memsize",
            "hw.physicalcpu",
            "hw.logicalcpu",
            "machdep.cpu.brand_string",
            "hw.model",
        ])?;
        let total_unified_memory_bytes = parse_u64("hw.memsize", &values[0])?;
        let physical_cpu_cores = parse_u32("hw.physicalcpu", &values[1])?;
        let logical_cpu_cores = parse_u32("hw.logicalcpu", &values[2])?;
        let model_storage_available_bytes = available_storage_bytes(model_storage_path)?;

        Ok(HardwareProfile {
            chip: values[3].clone(),
            model_identifier: values[4].clone(),
            total_unified_memory_bytes,
            physical_cpu_cores,
            logical_cpu_cores,
            model_storage_path: model_storage_path.display().to_string(),
            model_storage_available_bytes,
            recommendation: recommendation_for_memory(total_unified_memory_bytes),
        })
    }
}

fn sysctl_values(fields: &[&'static str]) -> Result<Vec<String>, HardwareProbeError> {
    let output = Command::new("/usr/sbin/sysctl")
        .arg("-n")
        .args(fields)
        .output()
        .map_err(|error| HardwareProbeError::SystemField {
            field: "sysctl",
            message: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(HardwareProbeError::SystemField {
            field: "sysctl",
            message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    let values = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.len() != fields.len() || values.iter().any(String::is_empty) {
        return Err(HardwareProbeError::SystemField {
            field: "sysctl",
            message: "系统返回的字段数量不完整".to_owned(),
        });
    }
    Ok(values)
}

fn parse_u64(field: &'static str, value: &str) -> Result<u64, HardwareProbeError> {
    value
        .parse()
        .map_err(|_| HardwareProbeError::InvalidNumber { field })
}

fn parse_u32(field: &'static str, value: &str) -> Result<u32, HardwareProbeError> {
    value
        .parse()
        .map_err(|_| HardwareProbeError::InvalidNumber { field })
}

fn available_storage_bytes(path: &Path) -> Result<u64, HardwareProbeError> {
    let output = Command::new("/bin/df")
        .args(["-Pk"])
        .arg(path)
        .output()
        .map_err(|error| HardwareProbeError::Storage(error.to_string()))?;
    if !output.status.success() {
        return Err(HardwareProbeError::Storage(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    parse_available_storage_bytes(&String::from_utf8_lossy(&output.stdout))
}

fn parse_available_storage_bytes(output: &str) -> Result<u64, HardwareProbeError> {
    let available_kib = output
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| HardwareProbeError::Storage("df 输出格式无法识别".to_owned()))?;
    available_kib
        .checked_mul(1024)
        .ok_or_else(|| HardwareProbeError::Storage("可用空间数值溢出".to_owned()))
}

fn recommendation_for_memory(total_bytes: u64) -> HardwareRecommendation {
    const GIB: u64 = 1024 * 1024 * 1024;
    let total_gib = total_bytes / GIB;
    let (summary, parameter_range, conservative_model_gib) = match total_gib {
        0..=8 => ("优先轻量模型", "1B–3B", 4),
        9..=16 => ("适合日常本地推理", "3B–8B", 9),
        17..=24 => ("适合中型模型", "7B–14B", 15),
        25..=36 => ("可尝试较大模型", "8B–20B", 22),
        37..=64 => ("适合较大本地模型", "14B–32B", 40),
        _ => ("可运行大型量化模型", "32B–70B", 56),
    };
    HardwareRecommendation {
        summary: summary.to_owned(),
        parameter_range: parameter_range.to_owned(),
        quantization: "优先 GGUF Q4_K_M；需要质量时再评估 Q5_K_M".to_owned(),
        conservative_model_bytes: conservative_model_gib * GIB,
        notes: vec![
            "建议值为保守起点，实际占用还取决于上下文长度和 KV Cache。".to_owned(),
            "HAL100 不会在后台持续采样硬件信息。".to_owned(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_compiled_platform_without_runtime_polling() {
        let summary = MacOsSystemProbe.platform_summary();

        assert!(!summary.os.is_empty());
        assert!(!summary.architecture.is_empty());
    }

    #[test]
    fn parses_posix_df_available_space() {
        let output = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3 100 20 80 20% /tmp\n";
        assert_eq!(
            parse_available_storage_bytes(output).expect("available"),
            81_920
        );
    }

    #[test]
    fn recommends_a_conservative_range_for_sixteen_gib() {
        let recommendation = recommendation_for_memory(16 * 1024 * 1024 * 1024);
        assert_eq!(recommendation.parameter_range, "3B–8B");
        assert_eq!(
            recommendation.conservative_model_bytes,
            9 * 1024 * 1024 * 1024
        );
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn probes_real_apple_silicon_hardware_on_demand() {
        let profile = MacOsSystemProbe
            .hardware_profile(Path::new("/tmp"))
            .expect("hardware profile");
        assert!(profile.chip.starts_with("Apple "));
        assert!(profile.total_unified_memory_bytes >= 8 * 1024 * 1024 * 1024);
        assert!(profile.model_storage_available_bytes > 0);
    }
}
