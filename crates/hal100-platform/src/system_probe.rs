use std::path::Path;

use hal100_protocol::{InferenceAccelerator, InferenceArchitecture, InferencePlatform};

use crate::HardwareProbeError;

pub(crate) struct HostProbeFacts {
    pub platform: InferencePlatform,
    pub architecture: InferenceArchitecture,
    pub cpu_brand: String,
    pub device_model: String,
    pub total_memory_bytes: u64,
    pub physical_cpu_cores: u32,
    pub logical_cpu_cores: u32,
    pub accelerators: Vec<InferenceAccelerator>,
}

pub(crate) fn compiled_host_identity() -> Option<(InferencePlatform, InferenceArchitecture)> {
    let platform = if cfg!(target_os = "macos") {
        InferencePlatform::MacOs
    } else if cfg!(target_os = "windows") {
        InferencePlatform::Windows
    } else if cfg!(target_os = "linux") {
        InferencePlatform::Linux
    } else {
        return None;
    };
    let architecture = if cfg!(target_arch = "aarch64") {
        InferenceArchitecture::Aarch64
    } else if cfg!(target_arch = "x86_64") {
        InferenceArchitecture::X86_64
    } else {
        return None;
    };
    Some((platform, architecture))
}

/// Map bounded Windows GPU/NPU PCI identifiers to conservative accelerator candidates.
///
/// This is only host hardware evidence: the corresponding engine/runtime still has to pass its
/// own read-only qualification before a support cell can be used. Unknown vendors are ignored.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn accelerators_from_windows_device_ids(
    video_controller_ids: &[String],
    compute_accelerator_ids: &[String],
) -> Vec<InferenceAccelerator> {
    let mut has_cuda = false;
    let mut has_rocm = false;
    let mut has_intel_gpu = false;
    let mut has_vulkan = false;
    for id in video_controller_ids {
        let normalized = id.to_ascii_uppercase();
        if normalized.contains("VEN_10DE") {
            has_cuda = true;
            has_vulkan = true;
        }
        if normalized.contains("VEN_1002") {
            has_rocm = true;
            has_vulkan = true;
        }
        if normalized.contains("VEN_8086") {
            has_intel_gpu = true;
            has_vulkan = true;
        }
    }
    let has_intel_npu = compute_accelerator_ids
        .iter()
        .any(|id| id.to_ascii_uppercase().contains("VEN_8086"));
    let mut accelerators = vec![InferenceAccelerator::Cpu];
    if has_cuda {
        accelerators.push(InferenceAccelerator::Cuda);
    }
    if has_rocm {
        accelerators.push(InferenceAccelerator::Rocm);
    }
    if has_vulkan {
        accelerators.push(InferenceAccelerator::Vulkan);
    }
    if has_intel_gpu {
        accelerators.push(InferenceAccelerator::IntelGpu);
    }
    if has_intel_npu {
        accelerators.push(InferenceAccelerator::IntelNpu);
    }
    accelerators
}

/// Map bounded Linux device evidence to conservative accelerator candidates.
///
/// The result describes host capabilities only. A matching engine still has to pass its own
/// runtime qualification, so the presence of a DRM render node or `/dev/kfd` never promotes a
/// support cell by itself.
#[cfg(any(target_os = "linux", test))]
pub(crate) fn accelerators_from_linux_device_evidence(
    has_nvidia_cuda: bool,
    has_amd_rocm: bool,
    has_intel_gpu: bool,
    has_intel_npu: bool,
    has_vulkan: bool,
) -> Vec<InferenceAccelerator> {
    let mut accelerators = vec![InferenceAccelerator::Cpu];
    if has_nvidia_cuda {
        accelerators.push(InferenceAccelerator::Cuda);
    }
    if has_amd_rocm {
        accelerators.push(InferenceAccelerator::Rocm);
    }
    if has_vulkan {
        accelerators.push(InferenceAccelerator::Vulkan);
    }
    if has_intel_gpu {
        accelerators.push(InferenceAccelerator::IntelGpu);
    }
    if has_intel_npu {
        accelerators.push(InferenceAccelerator::IntelNpu);
    }
    accelerators
}

pub(crate) fn probe_host() -> Result<HostProbeFacts, HardwareProbeError> {
    platform::probe_host()
}

pub(crate) fn total_memory_bytes() -> Result<u64, HardwareProbeError> {
    platform::total_memory_bytes()
}

pub(crate) fn available_storage_bytes(path: &Path) -> Result<u64, HardwareProbeError> {
    platform::available_storage_bytes(path)
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn parse_u64(field: &'static str, value: &str) -> Result<u64, HardwareProbeError> {
    value
        .trim()
        .parse()
        .map_err(|_| HardwareProbeError::InvalidNumber { field })
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn parse_u32(field: &'static str, value: &str) -> Result<u32, HardwareProbeError> {
    value
        .trim()
        .parse()
        .map_err(|_| HardwareProbeError::InvalidNumber { field })
}

#[cfg(any(target_os = "macos", target_os = "linux", test))]
fn parse_posix_df_available_space(output: &str) -> Result<u64, HardwareProbeError> {
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

#[cfg(any(target_os = "linux", test))]
pub(crate) fn nvidia_cuda_evidence_from(driver_version: &str, pci_vendors: &[String]) -> bool {
    driver_version.len() <= 16 * 1024
        && driver_version
            .lines()
            .any(|line| line.contains("NVRM version:"))
        && pci_vendors
            .iter()
            .any(|vendor| vendor.trim().eq_ignore_ascii_case("0x10de"))
}

#[cfg(target_os = "macos")]
mod platform {
    use std::{
        io::{self, Read},
        path::Path,
        process::{Command, ExitStatus, Stdio},
        thread,
        time::{Duration, Instant},
    };

    use hal100_protocol::{InferenceAccelerator, InferenceArchitecture, InferencePlatform};
    use serde_json::Value;

    use super::{
        HardwareProbeError, HostProbeFacts, parse_posix_df_available_space, parse_u32, parse_u64,
    };

    const MAX_SYSTEM_PROFILER_BYTES: usize = 256 * 1024;
    const MAX_SYSTEM_PROFILER_ERROR_BYTES: usize = 16 * 1024;
    const SYSTEM_PROFILER_TIMEOUT: Duration = Duration::from_secs(5);

    pub(super) struct BoundedCommandOutput {
        pub(super) status: ExitStatus,
        pub(super) stdout: Vec<u8>,
        pub(super) stderr: Vec<u8>,
    }

    struct BoundedRead {
        bytes: Vec<u8>,
        overflowed: bool,
    }

    pub(super) fn probe_host() -> Result<HostProbeFacts, HardwareProbeError> {
        let architecture = if cfg!(target_arch = "aarch64") {
            InferenceArchitecture::Aarch64
        } else if cfg!(target_arch = "x86_64") {
            InferenceArchitecture::X86_64
        } else {
            return Err(HardwareProbeError::UnsupportedPlatform);
        };
        let values = sysctl_values(&[
            "hw.memsize",
            "hw.physicalcpu",
            "hw.logicalcpu",
            "machdep.cpu.brand_string",
            "hw.model",
        ])?;
        let mut accelerators = vec![InferenceAccelerator::Cpu];
        if architecture == InferenceArchitecture::Aarch64 || probe_metal_support()? {
            accelerators.push(InferenceAccelerator::Metal);
        }
        Ok(HostProbeFacts {
            platform: InferencePlatform::MacOs,
            architecture,
            cpu_brand: values[3].clone(),
            device_model: values[4].clone(),
            total_memory_bytes: parse_u64("hw.memsize", &values[0])?,
            physical_cpu_cores: parse_u32("hw.physicalcpu", &values[1])?,
            logical_cpu_cores: parse_u32("hw.logicalcpu", &values[2])?,
            accelerators,
        })
    }

    fn probe_metal_support() -> Result<bool, HardwareProbeError> {
        let mut command = Command::new("/usr/sbin/system_profiler");
        command.args(["SPDisplaysDataType", "-json", "-detailLevel", "mini"]);
        let output = run_bounded_command(
            &mut command,
            "system_profiler",
            MAX_SYSTEM_PROFILER_BYTES,
            MAX_SYSTEM_PROFILER_ERROR_BYTES,
            SYSTEM_PROFILER_TIMEOUT,
        )?;
        if !output.status.success() {
            return Err(HardwareProbeError::SystemField {
                field: "system_profiler",
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        metal_supported_from_system_profiler(&output.stdout)
    }

    pub(super) fn run_bounded_command(
        command: &mut Command,
        field: &'static str,
        stdout_limit: usize,
        stderr_limit: usize,
        timeout: Duration,
    ) -> Result<BoundedCommandOutput, HardwareProbeError> {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| HardwareProbeError::SystemField {
                field,
                message: error.to_string(),
            })?;
        let stdout = child
            .stdout
            .take()
            .expect("piped stdout must exist after a successful spawn");
        let stderr = child
            .stderr
            .take()
            .expect("piped stderr must exist after a successful spawn");
        let stdout_reader = thread::spawn(move || read_bounded(stdout, stdout_limit));
        let stderr_reader = thread::spawn(move || read_bounded(stderr, stderr_limit));
        let deadline = Instant::now() + timeout;
        let (status, timed_out, wait_error) = loop {
            match child.try_wait() {
                Ok(Some(status)) => break (status, false, None),
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    let _ = child.kill();
                    let status = child
                        .wait()
                        .map_err(|error| HardwareProbeError::SystemField {
                            field,
                            message: error.to_string(),
                        })?;
                    break (status, true, None);
                }
                Err(error) => {
                    let _ = child.kill();
                    let status =
                        child
                            .wait()
                            .map_err(|wait_error| HardwareProbeError::SystemField {
                                field,
                                message: wait_error.to_string(),
                            })?;
                    break (status, false, Some(error));
                }
            }
        };
        let stdout = join_bounded_reader(stdout_reader, field)?;
        let stderr = join_bounded_reader(stderr_reader, field)?;
        if let Some(error) = wait_error {
            return Err(HardwareProbeError::SystemField {
                field,
                message: error.to_string(),
            });
        }
        if timed_out {
            return Err(HardwareProbeError::SystemField {
                field,
                message: "系统命令超过固定执行时限".to_owned(),
            });
        }
        if stdout.overflowed || stderr.overflowed {
            return Err(HardwareProbeError::SystemField {
                field,
                message: "系统命令输出超过固定读取上限".to_owned(),
            });
        }
        Ok(BoundedCommandOutput {
            status,
            stdout: stdout.bytes,
            stderr: stderr.bytes,
        })
    }

    fn read_bounded<R: Read>(mut reader: R, limit: usize) -> io::Result<BoundedRead> {
        let mut bytes = Vec::with_capacity(limit.min(16 * 1024));
        reader
            .by_ref()
            .take(limit.saturating_add(1) as u64)
            .read_to_end(&mut bytes)?;
        let overflowed = bytes.len() > limit;
        if overflowed {
            bytes.truncate(limit);
            io::copy(&mut reader, &mut io::sink())?;
        }
        Ok(BoundedRead { bytes, overflowed })
    }

    fn join_bounded_reader(
        reader: thread::JoinHandle<io::Result<BoundedRead>>,
        field: &'static str,
    ) -> Result<BoundedRead, HardwareProbeError> {
        reader
            .join()
            .map_err(|_| HardwareProbeError::SystemField {
                field,
                message: "系统命令输出读取线程异常".to_owned(),
            })?
            .map_err(|error| HardwareProbeError::SystemField {
                field,
                message: error.to_string(),
            })
    }

    pub(super) fn metal_supported_from_system_profiler(
        output: &[u8],
    ) -> Result<bool, HardwareProbeError> {
        if output.len() > MAX_SYSTEM_PROFILER_BYTES {
            return Err(HardwareProbeError::SystemField {
                field: "system_profiler",
                message: "显示设备输出超过上限".to_owned(),
            });
        }
        let value = serde_json::from_slice::<Value>(output).map_err(|error| {
            HardwareProbeError::SystemField {
                field: "system_profiler",
                message: error.to_string(),
            }
        })?;
        Ok(value
            .get("SPDisplaysDataType")
            .and_then(Value::as_array)
            .is_some_and(|devices| {
                devices.iter().any(|device| {
                    device
                        .get("spdisplays_mtlgpufamilysupport")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.starts_with("spdisplays_metal"))
                })
            }))
    }

    pub(super) fn total_memory_bytes() -> Result<u64, HardwareProbeError> {
        let values = sysctl_values(&["hw.memsize"])?;
        parse_u64("hw.memsize", &values[0])
    }

    pub(super) fn available_storage_bytes(path: &Path) -> Result<u64, HardwareProbeError> {
        posix_available_storage_bytes("/bin/df", path)
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

    fn posix_available_storage_bytes(binary: &str, path: &Path) -> Result<u64, HardwareProbeError> {
        let output = Command::new(binary)
            .args(["-Pk"])
            .arg(path)
            .output()
            .map_err(|error| HardwareProbeError::Storage(error.to_string()))?;
        if !output.status.success() {
            return Err(HardwareProbeError::Storage(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        parse_posix_df_available_space(&String::from_utf8_lossy(&output.stdout))
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::{collections::HashSet, fs, io::Read, path::Path, process::Command};

    use hal100_protocol::{InferenceAccelerator, InferencePlatform};

    use super::{
        HardwareProbeError, HostProbeFacts, compiled_host_identity, parse_posix_df_available_space,
    };

    pub(super) fn probe_host() -> Result<HostProbeFacts, HardwareProbeError> {
        let (_, architecture) =
            compiled_host_identity().ok_or(HardwareProbeError::UnsupportedPlatform)?;
        let cpuinfo = read_text("/proc/cpuinfo", "proc.cpuinfo")?;
        let logical_cpu_cores = std::thread::available_parallelism()
            .map(|value| u32::try_from(value.get()).unwrap_or(u32::MAX))
            .map_err(|error| HardwareProbeError::SystemField {
                field: "logical_cpu_cores",
                message: error.to_string(),
            })?;
        let accelerators = linux_accelerators();
        Ok(HostProbeFacts {
            platform: InferencePlatform::Linux,
            architecture,
            cpu_brand: linux_cpu_brand(&cpuinfo),
            device_model: linux_device_model(),
            total_memory_bytes: total_memory_bytes()?,
            physical_cpu_cores: linux_physical_cores(&cpuinfo).unwrap_or(logical_cpu_cores),
            logical_cpu_cores,
            // CUDA is promoted only when a loaded NVIDIA kernel driver and a matching DRM PCI
            // vendor are both present. Engine-level qualification must still prove the runtime.
            accelerators,
        })
    }

    pub(super) fn total_memory_bytes() -> Result<u64, HardwareProbeError> {
        let meminfo = read_text("/proc/meminfo", "proc.meminfo")?;
        let kib = meminfo
            .lines()
            .find_map(|line| line.strip_prefix("MemTotal:"))
            .and_then(|value| value.split_whitespace().next())
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or(HardwareProbeError::InvalidNumber {
                field: "proc.meminfo.MemTotal",
            })?;
        kib.checked_mul(1024)
            .ok_or(HardwareProbeError::InvalidNumber {
                field: "proc.meminfo.MemTotal",
            })
    }

    pub(super) fn available_storage_bytes(path: &Path) -> Result<u64, HardwareProbeError> {
        let binary = if Path::new("/bin/df").is_file() {
            "/bin/df"
        } else if Path::new("/usr/bin/df").is_file() {
            "/usr/bin/df"
        } else {
            return Err(HardwareProbeError::Storage(
                "系统未提供固定路径的df".to_owned(),
            ));
        };
        let output = Command::new(binary)
            .args(["-Pk"])
            .arg(path)
            .output()
            .map_err(|error| HardwareProbeError::Storage(error.to_string()))?;
        if !output.status.success() {
            return Err(HardwareProbeError::Storage(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        parse_posix_df_available_space(&String::from_utf8_lossy(&output.stdout))
    }

    fn read_text(path: &str, field: &'static str) -> Result<String, HardwareProbeError> {
        read_text_bounded(path, field, 4 * 1024 * 1024)
    }

    fn read_text_bounded(
        path: &str,
        field: &'static str,
        limit: u64,
    ) -> Result<String, HardwareProbeError> {
        let file = fs::File::open(path).map_err(|error| HardwareProbeError::SystemField {
            field,
            message: error.to_string(),
        })?;
        let mut value = String::new();
        file.take(limit.saturating_add(1))
            .read_to_string(&mut value)
            .map_err(|error| HardwareProbeError::SystemField {
                field,
                message: error.to_string(),
            })?;
        if value.len() as u64 > limit {
            return Err(HardwareProbeError::SystemField {
                field,
                message: "系统字段超过固定读取上限".to_owned(),
            });
        }
        Ok(value)
    }

    fn linux_accelerators() -> Vec<InferenceAccelerator> {
        const MAX_DRM_ENTRIES: usize = 256;
        const MAX_ACCEL_ENTRIES: usize = 64;
        let driver_version = read_text_bounded(
            "/proc/driver/nvidia/version",
            "nvidia.driver.version",
            16 * 1024,
        )
        .ok();
        let mut vendors = Vec::new();
        if let Ok(entries) = fs::read_dir("/sys/class/drm") {
            for (index, entry) in entries.enumerate() {
                if index >= MAX_DRM_ENTRIES {
                    vendors.clear();
                    break;
                }
                let Ok(entry) = entry else {
                    continue;
                };
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if !name.starts_with("card") || name.contains('-') {
                    continue;
                }
                if let Ok(vendor) = fs::read_to_string(entry.path().join("device/vendor"))
                    && vendor.len() <= 32
                {
                    vendors.push(vendor);
                }
            }
        }
        let has_nvidia_cuda = driver_version
            .as_deref()
            .is_some_and(|driver| super::nvidia_cuda_evidence_from(driver, &vendors));
        let has_render_node = Path::new("/dev/dri")
            .read_dir()
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("renderD"))
            });
        let has_amd_rocm = has_render_node
            && Path::new("/dev/kfd").exists()
            && vendors
                .iter()
                .any(|vendor| vendor.trim().eq_ignore_ascii_case("0x1002"));
        let has_intel_gpu = has_render_node
            && vendors
                .iter()
                .any(|vendor| vendor.trim().eq_ignore_ascii_case("0x8086"));
        let mut has_intel_npu = false;
        if let Ok(entries) = fs::read_dir("/sys/class/accel") {
            for (index, entry) in entries.enumerate() {
                if index >= MAX_ACCEL_ENTRIES {
                    has_intel_npu = false;
                    break;
                }
                let Ok(entry) = entry else {
                    continue;
                };
                let name = entry.file_name();
                let Some(name) = name.to_str() else {
                    continue;
                };
                if !name.starts_with("accel") {
                    continue;
                }
                if fs::read_to_string(entry.path().join("device/vendor"))
                    .ok()
                    .filter(|vendor| vendor.len() <= 32)
                    .is_some_and(|vendor| vendor.trim().eq_ignore_ascii_case("0x8086"))
                {
                    has_intel_npu = Path::new("/dev/accel").is_dir();
                    if has_intel_npu {
                        break;
                    }
                }
            }
        }
        let has_vulkan = has_render_node
            && vendors.iter().any(|vendor| {
                matches!(
                    vendor.trim().to_ascii_lowercase().as_str(),
                    "0x10de" | "0x1002" | "0x8086"
                )
            });
        super::accelerators_from_linux_device_evidence(
            has_nvidia_cuda,
            has_amd_rocm,
            has_intel_gpu,
            has_intel_npu,
            has_vulkan,
        )
    }

    fn linux_cpu_brand(cpuinfo: &str) -> String {
        cpuinfo
            .lines()
            .find_map(|line| {
                let (key, value) = line.split_once(':')?;
                matches!(key.trim(), "model name" | "Hardware" | "Processor")
                    .then(|| value.trim().to_owned())
            })
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Linux CPU".to_owned())
    }

    fn linux_device_model() -> String {
        [
            "/sys/devices/virtual/dmi/id/product_name",
            "/proc/device-tree/model",
        ]
        .into_iter()
        .find_map(|path| fs::read_to_string(path).ok())
        .map(|value| value.trim_matches(['\0', '\n', '\r', ' ']).to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "Linux host".to_owned())
    }

    fn linux_physical_cores(cpuinfo: &str) -> Option<u32> {
        let mut sockets = HashSet::<(String, String)>::new();
        for block in cpuinfo.split("\n\n") {
            let mut physical_id = None;
            let mut core_id = None;
            for line in block.lines() {
                let Some((key, value)) = line.split_once(':') else {
                    continue;
                };
                match key.trim() {
                    "physical id" => physical_id = Some(value.trim().to_owned()),
                    "core id" => core_id = Some(value.trim().to_owned()),
                    _ => {}
                }
            }
            if let (Some(physical_id), Some(core_id)) = (physical_id, core_id) {
                sockets.insert((physical_id, core_id));
            }
        }
        (!sockets.is_empty()).then(|| u32::try_from(sockets.len()).unwrap_or(u32::MAX))
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::{path::Path, process::Command};

    use hal100_protocol::InferencePlatform;

    use super::{
        HardwareProbeError, HostProbeFacts, accelerators_from_windows_device_ids,
        compiled_host_identity, parse_u32, parse_u64,
    };

    const POWERSHELL: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

    pub(super) fn probe_host() -> Result<HostProbeFacts, HardwareProbeError> {
        let (_, architecture) =
            compiled_host_identity().ok_or(HardwareProbeError::UnsupportedPlatform)?;
        let values = powershell_lines(
            "$c=Get-CimInstance Win32_ComputerSystem;\
             $p=Get-CimInstance Win32_Processor | Select-Object -First 1;\
             [Console]::WriteLine($c.TotalPhysicalMemory);\
             [Console]::WriteLine($p.NumberOfCores);\
             [Console]::WriteLine($p.NumberOfLogicalProcessors);\
             [Console]::WriteLine($p.Name);\
             [Console]::WriteLine($c.Model)",
            "windows.hardware",
        )?;
        if values.len() != 5 {
            return Err(HardwareProbeError::SystemField {
                field: "windows.hardware",
                message: "系统返回的字段数量不完整".to_owned(),
            });
        }
        let video_controller_ids = powershell_lines(
            "$v=Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty PNPDeviceID;\
             if($v){$v | Select-Object -First 32}",
            "windows.video_controllers",
        )?;
        let compute_accelerator_ids = powershell_lines(
            "$n=Get-CimInstance Win32_PnPEntity -Filter \"PNPClass='ComputeAccelerator'\" | Select-Object -ExpandProperty PNPDeviceID;\
             if($n){$n | Select-Object -First 32}",
            "windows.compute_accelerators",
        )
        .unwrap_or_default();
        Ok(HostProbeFacts {
            platform: InferencePlatform::Windows,
            architecture,
            cpu_brand: values[3].clone(),
            device_model: values[4].clone(),
            total_memory_bytes: parse_u64("windows.TotalPhysicalMemory", &values[0])?,
            physical_cpu_cores: parse_u32("windows.NumberOfCores", &values[1])?,
            logical_cpu_cores: parse_u32("windows.NumberOfLogicalProcessors", &values[2])?,
            accelerators: accelerators_from_windows_device_ids(
                &video_controller_ids,
                &compute_accelerator_ids,
            ),
        })
    }

    pub(super) fn total_memory_bytes() -> Result<u64, HardwareProbeError> {
        let values = powershell_lines(
            "$c=Get-CimInstance Win32_ComputerSystem;[Console]::WriteLine($c.TotalPhysicalMemory)",
            "windows.TotalPhysicalMemory",
        )?;
        values
            .first()
            .ok_or(HardwareProbeError::InvalidNumber {
                field: "windows.TotalPhysicalMemory",
            })
            .and_then(|value| parse_u64("windows.TotalPhysicalMemory", value))
    }

    pub(super) fn available_storage_bytes(path: &Path) -> Result<u64, HardwareProbeError> {
        let output = Command::new(POWERSHELL)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$p=[IO.Path]::GetFullPath($env:HAL100_PROBE_STORAGE_PATH);\
                 $r=[IO.Path]::GetPathRoot($p);\
                 $d=[IO.DriveInfo]::new($r);\
                 [Console]::WriteLine($d.AvailableFreeSpace)",
            ])
            .env("HAL100_PROBE_STORAGE_PATH", path)
            .output()
            .map_err(|error| HardwareProbeError::Storage(error.to_string()))?;
        if !output.status.success() {
            return Err(HardwareProbeError::Storage(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        parse_u64(
            "windows.AvailableFreeSpace",
            String::from_utf8_lossy(&output.stdout).trim(),
        )
    }

    fn powershell_lines(
        script: &str,
        field: &'static str,
    ) -> Result<Vec<String>, HardwareProbeError> {
        let output = Command::new(POWERSHELL)
            .args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                script,
            ])
            .output()
            .map_err(|error| HardwareProbeError::SystemField {
                field,
                message: error.to_string(),
            })?;
        if !output.status.success() {
            return Err(HardwareProbeError::SystemField {
                field,
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
mod platform {
    use std::path::Path;

    use super::{HardwareProbeError, HostProbeFacts};

    pub(super) fn probe_host() -> Result<HostProbeFacts, HardwareProbeError> {
        Err(HardwareProbeError::UnsupportedPlatform)
    }

    pub(super) fn total_memory_bytes() -> Result<u64, HardwareProbeError> {
        Err(HardwareProbeError::UnsupportedPlatform)
    }

    pub(super) fn available_storage_bytes(_path: &Path) -> Result<u64, HardwareProbeError> {
        Err(HardwareProbeError::UnsupportedPlatform)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn compiled_identity_is_bounded_to_the_three_target_platforms_and_two_architectures() {
        let identity = compiled_host_identity();
        if cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        )) && cfg!(any(target_arch = "aarch64", target_arch = "x86_64"))
        {
            assert!(identity.is_some());
        } else {
            assert!(identity.is_none());
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_metal_probe_requires_the_bounded_system_profiler_capability_key() {
        let supported = br#"{
          "SPDisplaysDataType": [{
            "_name": "Intel Iris Plus Graphics",
            "spdisplays_mtlgpufamilysupport": "spdisplays_metal3"
          }]
        }"#;
        assert!(
            platform::metal_supported_from_system_profiler(supported)
                .expect("valid display profile")
        );
        assert!(
            !platform::metal_supported_from_system_profiler(
                br#"{"SPDisplaysDataType":[{"_name":"Legacy GPU"}]}"#
            )
            .expect("valid unsupported display profile")
        );
        assert!(platform::metal_supported_from_system_profiler(b"not-json").is_err());
        assert!(
            platform::metal_supported_from_system_profiler(&vec![b' '; 256 * 1024 + 1]).is_err()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_bounded_command_caps_time_and_stream_bytes() {
        let mut success = std::process::Command::new("/bin/sh");
        success.args(["-c", "printf ok"]);
        let output = platform::run_bounded_command(
            &mut success,
            "test.command",
            32,
            32,
            std::time::Duration::from_secs(1),
        )
        .expect("bounded success");
        assert!(output.status.success());
        assert_eq!(output.stdout, b"ok");

        let mut overflow = std::process::Command::new("/bin/sh");
        overflow.args(["-c", "printf '%01024d' 0"]);
        assert!(
            platform::run_bounded_command(
                &mut overflow,
                "test.command",
                32,
                32,
                std::time::Duration::from_secs(1),
            )
            .is_err()
        );

        let started = std::time::Instant::now();
        let mut timeout = std::process::Command::new("/bin/sh");
        timeout.args(["-c", "sleep 1"]);
        assert!(
            platform::run_bounded_command(
                &mut timeout,
                "test.command",
                32,
                32,
                std::time::Duration::from_millis(25),
            )
            .is_err()
        );
        assert!(started.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn parses_posix_df_available_space_without_locale_dependent_headers() {
        let output = "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/disk3 100 20 80 20% /tmp\n";
        assert_eq!(
            parse_posix_df_available_space(output).expect("available"),
            81_920
        );
    }

    #[test]
    fn maps_known_windows_gpu_and_npu_vendors_to_conservative_candidates() {
        let video_ids = vec![
            r"PCI\VEN_10DE&DEV_2684".to_owned(),
            r"PCI\VEN_1002&DEV_744C".to_owned(),
            r"PCI\VEN_8086&DEV_56A0".to_owned(),
            r"PCI\VEN_1234&DEV_1111".to_owned(),
        ];
        let compute_ids = vec![r"PCI\VEN_8086&DEV_7D1D".to_owned()];
        assert_eq!(
            accelerators_from_windows_device_ids(&video_ids, &compute_ids),
            vec![
                InferenceAccelerator::Cpu,
                InferenceAccelerator::Cuda,
                InferenceAccelerator::Rocm,
                InferenceAccelerator::Vulkan,
                InferenceAccelerator::IntelGpu,
                InferenceAccelerator::IntelNpu,
            ]
        );
    }

    #[test]
    fn unknown_or_duplicate_windows_video_controller_ids_do_not_expand_candidates() {
        let ids = vec![
            r"PCI\VEN_1234&DEV_1111".to_owned(),
            r"PCI\VEN_10DE&DEV_2684".to_owned(),
            r"PCI\VEN_10DE&DEV_2685".to_owned(),
        ];
        let candidates = accelerators_from_windows_device_ids(&ids, &[]);
        assert_eq!(
            candidates,
            vec![
                InferenceAccelerator::Cpu,
                InferenceAccelerator::Cuda,
                InferenceAccelerator::Vulkan,
            ]
        );
        assert_eq!(
            candidates.iter().collect::<HashSet<_>>().len(),
            candidates.len()
        );
    }

    #[test]
    fn maps_linux_runtime_evidence_to_conservative_candidates() {
        assert_eq!(
            accelerators_from_linux_device_evidence(true, true, true, true, true),
            vec![
                InferenceAccelerator::Cpu,
                InferenceAccelerator::Cuda,
                InferenceAccelerator::Rocm,
                InferenceAccelerator::Vulkan,
                InferenceAccelerator::IntelGpu,
                InferenceAccelerator::IntelNpu,
            ]
        );
        assert_eq!(
            accelerators_from_linux_device_evidence(false, false, false, false, false),
            vec![InferenceAccelerator::Cpu]
        );
    }
}
