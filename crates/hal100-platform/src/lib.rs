mod keychain;
mod sidecar_launch;
mod system_probe;

use std::path::Path;

use hal100_core::SystemProbe;
use hal100_protocol::{
    HardwareProfile, HardwareRecommendation, HostCapabilitySnapshot, PlatformSummary,
};
use thiserror::Error;

pub use keychain::{DEFAULT_KEYCHAIN_SERVICE, MacOsKeychainSecretStore};
pub use sidecar_launch::{
    AgentKernelLaunchSpec, SidecarIsolation, SidecarLaunchError, prepare_agent_kernel_command,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct NativeSystemProbe;

#[derive(Debug, Error)]
pub enum HardwareProbeError {
    #[error("当前构建尚未实现此平台的完整硬件能力检测")]
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

impl SystemProbe for NativeSystemProbe {
    fn platform_summary(&self) -> PlatformSummary {
        PlatformSummary {
            os: compiled_platform_name().to_owned(),
            architecture: compiled_architecture_name().to_owned(),
            supported: system_probe::compiled_host_identity().is_some(),
        }
    }
}

impl NativeSystemProbe {
    /// Produces one immutable capability snapshot for compatibility and policy decisions.
    /// The probe is invoked on demand and never starts a sampler or retains a device identifier.
    pub fn capability_snapshot(
        &self,
        model_storage_path: &Path,
    ) -> Result<HostCapabilitySnapshot, HardwareProbeError> {
        let facts = system_probe::probe_host()?;
        Ok(HostCapabilitySnapshot {
            platform: facts.platform,
            architecture: facts.architecture,
            cpu_brand: facts.cpu_brand,
            device_model: facts.device_model,
            total_memory_bytes: facts.total_memory_bytes,
            physical_cpu_cores: facts.physical_cpu_cores,
            logical_cpu_cores: facts.logical_cpu_cores,
            accelerators: facts.accelerators,
            model_storage_path: model_storage_path.display().to_string(),
            model_storage_available_bytes: system_probe::available_storage_bytes(
                model_storage_path,
            )?,
            probe_revision: "host-capabilities-v3".to_owned(),
        })
    }

    /// Reads unified memory once for startup policy selection. This does not start a sampler or
    /// retain any hardware identifier.
    pub fn total_unified_memory_bytes(&self) -> Result<u64, HardwareProbeError> {
        system_probe::total_memory_bytes()
    }

    pub fn model_storage_available_bytes(
        &self,
        model_storage_path: &Path,
    ) -> Result<u64, HardwareProbeError> {
        system_probe::available_storage_bytes(model_storage_path)
    }

    pub fn hardware_profile(
        &self,
        model_storage_path: &Path,
    ) -> Result<HardwareProfile, HardwareProbeError> {
        let snapshot = self.capability_snapshot(model_storage_path)?;

        Ok(Self::hardware_profile_from_capabilities(snapshot))
    }

    pub fn hardware_profile_from_capabilities(snapshot: HostCapabilitySnapshot) -> HardwareProfile {
        HardwareProfile {
            chip: snapshot.cpu_brand,
            model_identifier: snapshot.device_model,
            total_unified_memory_bytes: snapshot.total_memory_bytes,
            physical_cpu_cores: snapshot.physical_cpu_cores,
            logical_cpu_cores: snapshot.logical_cpu_cores,
            model_storage_path: snapshot.model_storage_path,
            model_storage_available_bytes: snapshot.model_storage_available_bytes,
            recommendation: recommendation_for_memory(snapshot.total_memory_bytes),
        }
    }
}

const fn compiled_platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else if cfg!(target_os = "linux") {
        "Linux"
    } else {
        std::env::consts::OS
    }
}

const fn compiled_architecture_name() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "Apple Silicon"
    } else {
        std::env::consts::ARCH
    }
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
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    use hal100_protocol::{InferenceAccelerator, InferenceArchitecture, InferencePlatform};

    use super::*;

    #[test]
    fn reports_the_compiled_platform_without_runtime_polling() {
        let summary = NativeSystemProbe.platform_summary();

        assert!(!summary.os.is_empty());
        assert!(!summary.architecture.is_empty());
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
    fn probes_total_memory_once_for_runtime_policy() {
        assert!(
            NativeSystemProbe
                .total_unified_memory_bytes()
                .expect("total unified memory")
                > 0
        );
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn probes_real_apple_silicon_hardware_on_demand() {
        let profile = NativeSystemProbe
            .hardware_profile(Path::new("/tmp"))
            .expect("hardware profile");
        assert!(profile.chip.starts_with("Apple "));
        assert!(profile.total_unified_memory_bytes > 0);
        assert!(profile.model_storage_available_bytes > 0);
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn capability_snapshot_reports_typed_platform_architecture_and_accelerators() {
        let snapshot = NativeSystemProbe
            .capability_snapshot(Path::new("/tmp"))
            .expect("host capabilities");
        assert_eq!(snapshot.platform, InferencePlatform::MacOs);
        assert_eq!(snapshot.architecture, InferenceArchitecture::Aarch64);
        assert_eq!(
            snapshot.accelerators,
            vec![InferenceAccelerator::Cpu, InferenceAccelerator::Metal]
        );
        assert_eq!(snapshot.probe_revision, "host-capabilities-v3");
    }

    #[test]
    fn linux_cuda_evidence_requires_both_nvidia_driver_and_pci_vendor() {
        let driver = "NVRM version: NVIDIA UNIX Open Kernel Module 580.65";
        assert!(system_probe::nvidia_cuda_evidence_from(
            driver,
            &["0x10de\n".to_owned()]
        ));
        assert!(!system_probe::nvidia_cuda_evidence_from(
            driver,
            &["0x8086\n".to_owned()]
        ));
        assert!(!system_probe::nvidia_cuda_evidence_from(
            "unknown driver",
            &["0x10de\n".to_owned()]
        ));
    }
}
