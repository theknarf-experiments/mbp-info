use std::ffi::CString;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[allow(deprecated)]
use libc::{
    c_void, host_processor_info, host_statistics64, mach_host_self, mach_msg_type_number_t,
    mach_task_self, processor_info_array_t, vm_deallocate, vm_statistics64_data_t, xsw_usage,
    HOST_VM_INFO64, KERN_SUCCESS, PROCESSOR_CPU_LOAD_INFO,
};
use once_cell::sync::Lazy;
use regex::Regex;

use crate::data::{ParsedInfo, ParsedMetric, SampleData};

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

static NUMERIC_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(-?\d+(?:\.\d+)?)(?:\s*([A-Za-z/%]+))?\s*$")
        .expect("valid regex")
});

pub trait Collector: Send {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn requires_sudo(&self) -> bool {
        false
    }
    fn interval(&self) -> Duration {
        SAMPLE_INTERVAL
    }
    fn run_once(&self) -> bool {
        false
    }
    fn sample(&mut self) -> Result<SampleData, String>;
}

pub enum CollectorMessage {
    Sample {
        data: SampleData,
    },
    Error {
        collector: &'static str,
        error: String,
    },
}

pub fn spawn_collector(
    mut collector: Box<dyn Collector>,
    tx: Sender<CollectorMessage>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    let id = collector.id();

    thread::spawn(move || {
        if collector.run_once() {
            if stop.load(Ordering::Relaxed) {
                match collector.sample() {
                    Ok(data) => {
                        let _ = tx.send(CollectorMessage::Sample { data });
                    }
                    Err(error) => {
                        let _ = tx.send(CollectorMessage::Error {
                            collector: id,
                            error,
                        });
                    }
                }
            }
            return;
        }

        loop {
            if !stop.load(Ordering::Relaxed) {
                break;
            }

            match collector.sample() {
                Ok(data) => {
                    let _ = tx.send(CollectorMessage::Sample { data });
                }
                Err(error) => {
                    let _ = tx.send(CollectorMessage::Error { collector: id, error });
                }
            }

            sleep_with_stop(collector.interval(), &stop);
        }
    })
}

fn sleep_with_stop(duration: Duration, stop: &AtomicBool) {
    let mut elapsed = Duration::from_millis(0);
    let step = Duration::from_millis(200);

    while elapsed < duration {
        if !stop.load(Ordering::Relaxed) {
            break;
        }
        let remaining = duration.saturating_sub(elapsed);
        let sleep_for = if remaining < step { remaining } else { step };
        thread::sleep(sleep_for);
        elapsed += sleep_for;
    }
}

pub fn needs_sudo() -> bool {
    let uid = unsafe { libc::geteuid() };
    uid != 0
}

fn parse_key_value_lines(text: &str) -> SampleData {
    let mut metrics = Vec::new();
    let mut info = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains(':') {
            continue;
        }

        let mut parts = trimmed.splitn(2, ':');
        let label = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();
        if label.is_empty() || rest.is_empty() {
            continue;
        }

        if let Some(caps) = NUMERIC_RE.captures(rest) {
            let value: f64 = match caps.get(1).map(|m| m.as_str().parse()) {
                Some(Ok(value)) => value,
                _ => continue,
            };
            let unit = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
            let key = format!("{}|{}", label, unit);

            metrics.push(ParsedMetric {
                key,
                label: label.to_string(),
                unit,
                value,
            });
        } else {
            info.push(ParsedInfo {
                label: label.to_string(),
                value: rest.to_string(),
            });
        }
    }

    SampleData { metrics, info }
}

fn run_command(command: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run {command}: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("{command} exited with status {}", output.status)
        } else {
            stderr
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

fn sysctl_string(name: &str) -> Result<String, String> {
    let cname = CString::new(name).map_err(|_| format!("invalid sysctl name: {name}"))?;
    let mut size: libc::size_t = 0;
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(format!("sysctl {name} failed"));
    }

    let mut buf = vec![0u8; size as usize];
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            buf.as_mut_ptr() as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(format!("sysctl {name} failed"));
    }

    let value = String::from_utf8_lossy(&buf[..size as usize]);
    Ok(value.trim_end_matches('\0').trim().to_string())
}

fn sysctl_u64(name: &str) -> Result<u64, String> {
    let cname = CString::new(name).map_err(|_| format!("invalid sysctl name: {name}"))?;
    let mut value: u64 = 0;
    let mut size = std::mem::size_of::<u64>();
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            &mut value as *mut _ as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(format!("sysctl {name} failed"));
    }

    Ok(value)
}

fn sysctl_timeval(name: &str) -> Result<libc::timeval, String> {
    let cname = CString::new(name).map_err(|_| format!("invalid sysctl name: {name}"))?;
    let mut value: libc::timeval = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<libc::timeval>();
    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            &mut value as *mut _ as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(format!("sysctl {name} failed"));
    }

    Ok(value)
}

fn bytes_to_mb(value: u64) -> f64 {
    value as f64 / (1024.0 * 1024.0)
}

pub struct PowermetricsCollector {
    use_sudo: bool,
}

impl PowermetricsCollector {
    pub fn new(use_sudo: bool) -> Self {
        Self { use_sudo }
    }
}

impl Collector for PowermetricsCollector {
    fn id(&self) -> &'static str {
        "powermetrics"
    }

    fn name(&self) -> &'static str {
        "Powermetrics (SMC)"
    }

    fn requires_sudo(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut command = if self.use_sudo {
            let mut command = Command::new("sudo");
            command.arg("-n").arg("powermetrics");
            command
        } else {
            Command::new("powermetrics")
        };

        let output = command
            .args(["--samplers", "smc", "-n", "1", "-i", "1000"])
            .output()
            .map_err(|error| format!("failed to run powermetrics: {error}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                format!("powermetrics exited with status {}", output.status)
            } else if self.use_sudo && stderr.to_lowercase().contains("password") {
                "sudo authentication required. Use :reauth powermetrics".to_string()
            } else {
                stderr
            };
            return Err(message);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}{stderr}");
        let data = parse_key_value_lines(&combined);

        if data.metrics.is_empty() && data.info.is_empty() {
            Err("no metrics parsed from powermetrics output".to_string())
        } else {
            Ok(data)
        }
    }
}

pub struct CpuUsageCollector {
    last_ticks: Option<Vec<[u64; 4]>>,
}

impl CpuUsageCollector {
    pub fn new() -> Self {
        Self { last_ticks: None }
    }
}

impl Collector for CpuUsageCollector {
    fn id(&self) -> &'static str {
        "cpu_usage"
    }

    fn name(&self) -> &'static str {
        "CPU Usage"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let current = read_cpu_ticks()?;
        let previous = match &self.last_ticks {
            Some(previous) => previous.clone(),
            None => {
                self.last_ticks = Some(current);
                return Ok(SampleData {
                    metrics: Vec::new(),
                    info: Vec::new(),
                });
            }
        };

        self.last_ticks = Some(current.clone());

        let mut metrics = Vec::new();
        for (index, (now, prev)) in current.iter().zip(previous.iter()).enumerate() {
            let deltas: Vec<u64> = now
                .iter()
                .zip(prev.iter())
                .map(|(n, p)| n.saturating_sub(*p))
                .collect();
            let total: u64 = deltas.iter().sum();
            if total == 0 {
                continue;
            }
            let used = deltas[0] + deltas[1] + deltas[3];
            let usage = (used as f64 / total as f64) * 100.0;
            let label = format!("CPU Core {} usage", index);
            metrics.push(ParsedMetric {
                key: format!("{}|%", label),
                label,
                unit: "%".to_string(),
                value: usage,
            });
        }

        Ok(SampleData {
            metrics,
            info: Vec::new(),
        })
    }
}

pub struct MemoryUsageCollector {
    page_size: u64,
}

impl MemoryUsageCollector {
    pub fn new() -> Self {
        let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) as u64 };
        Self { page_size }
    }
}

impl Collector for MemoryUsageCollector {
    fn id(&self) -> &'static str {
        "memory_usage"
    }

    fn name(&self) -> &'static str {
        "Memory Usage"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let vm_stat = read_vm_stats()?;

        let active = vm_stat.active_count as u64 * self.page_size;
        let inactive = vm_stat.inactive_count as u64 * self.page_size;
        let wired = vm_stat.wire_count as u64 * self.page_size;
        let compressed = vm_stat.compressor_page_count as u64 * self.page_size;
        let free =
            (vm_stat.free_count + vm_stat.speculative_count) as u64 * self.page_size;
        let used = active + inactive + wired + compressed;

        let metrics = vec![
            ParsedMetric {
                key: "Memory Used|MB".to_string(),
                label: "Memory Used".to_string(),
                unit: "MB".to_string(),
                value: bytes_to_mb(used),
            },
            ParsedMetric {
                key: "Memory Free|MB".to_string(),
                label: "Memory Free".to_string(),
                unit: "MB".to_string(),
                value: bytes_to_mb(free),
            },
            ParsedMetric {
                key: "Memory Wired|MB".to_string(),
                label: "Memory Wired".to_string(),
                unit: "MB".to_string(),
                value: bytes_to_mb(wired),
            },
            ParsedMetric {
                key: "Memory Compressed|MB".to_string(),
                label: "Memory Compressed".to_string(),
                unit: "MB".to_string(),
                value: bytes_to_mb(compressed),
            },
        ];

        Ok(SampleData {
            metrics,
            info: Vec::new(),
        })
    }
}

pub struct SwapUsageCollector;

impl SwapUsageCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for SwapUsageCollector {
    fn id(&self) -> &'static str {
        "swap_usage"
    }

    fn name(&self) -> &'static str {
        "Swap Usage"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let usage = read_swap_usage()?;

        let metrics = vec![
            ParsedMetric {
                key: "Swap Used|MB".to_string(),
                label: "Swap Used".to_string(),
                unit: "MB".to_string(),
                value: bytes_to_mb(usage.xsu_used),
            },
            ParsedMetric {
                key: "Swap Free|MB".to_string(),
                label: "Swap Free".to_string(),
                unit: "MB".to_string(),
                value: bytes_to_mb(usage.xsu_avail),
            },
            ParsedMetric {
                key: "Swap Total|MB".to_string(),
                label: "Swap Total".to_string(),
                unit: "MB".to_string(),
                value: bytes_to_mb(usage.xsu_total),
            },
        ];

        Ok(SampleData {
            metrics,
            info: Vec::new(),
        })
    }
}

pub struct UptimeCollector;

impl UptimeCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for UptimeCollector {
    fn id(&self) -> &'static str {
        "uptime"
    }

    fn name(&self) -> &'static str {
        "Uptime"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let boot_time = sysctl_timeval("kern.boottime")?;
        let boot = Duration::from_secs(boot_time.tv_sec as u64)
            + Duration::from_micros(boot_time.tv_usec as u64);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("time error: {error}"))?;
        let uptime = now.saturating_sub(boot);

        Ok(SampleData {
            metrics: vec![ParsedMetric {
                key: "Uptime|s".to_string(),
                label: "Uptime".to_string(),
                unit: "s".to_string(),
                value: uptime.as_secs_f64(),
            }],
            info: Vec::new(),
        })
    }
}

pub struct CpuInfoCollector;

impl CpuInfoCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for CpuInfoCollector {
    fn id(&self) -> &'static str {
        "cpu_info"
    }

    fn name(&self) -> &'static str {
        "CPU Info"
    }

    fn run_once(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut info = Vec::new();

        if let Ok(model) = sysctl_string("machdep.cpu.brand_string") {
            info.push(ParsedInfo {
                label: "CPU Brand".to_string(),
                value: model,
            });
        }

        info.push(ParsedInfo {
            label: "CPU Architecture".to_string(),
            value: std::env::consts::ARCH.to_string(),
        });

        if let Ok(logical) = sysctl_u64("hw.logicalcpu") {
            info.push(ParsedInfo {
                label: "CPU Cores (logical)".to_string(),
                value: logical.to_string(),
            });
        }

        if let Ok(physical) = sysctl_u64("hw.physicalcpu") {
            info.push(ParsedInfo {
                label: "CPU Cores (physical)".to_string(),
                value: physical.to_string(),
            });
        }

        if let Ok(freq) = sysctl_u64("hw.cpufrequency") {
            let ghz = freq as f64 / 1_000_000_000.0;
            info.push(ParsedInfo {
                label: "CPU Frequency".to_string(),
                value: format!("{ghz:.2} GHz"),
            });
        }

        if let Ok(model) = sysctl_string("hw.model") {
            info.push(ParsedInfo {
                label: "Machine Model".to_string(),
                value: model,
            });
        }

        Ok(SampleData {
            metrics: Vec::new(),
            info,
        })
    }
}

pub struct MemoryInfoCollector;

impl MemoryInfoCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for MemoryInfoCollector {
    fn id(&self) -> &'static str {
        "memory_info"
    }

    fn name(&self) -> &'static str {
        "Memory Info"
    }

    fn run_once(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut info = Vec::new();

        if let Ok(total) = sysctl_u64("hw.memsize") {
            let gb = total as f64 / (1024.0 * 1024.0 * 1024.0);
            info.push(ParsedInfo {
                label: "Memory Total".to_string(),
                value: format!("{gb:.2} GB"),
            });
        }

        if let Ok(output) = run_command("system_profiler", &["-detailLevel", "mini", "SPMemoryDataType"]) {
            for line in output.lines() {
                let trimmed = line.trim();
                if let Some(value) = trimmed.strip_prefix("Speed:") {
                    info.push(ParsedInfo {
                        label: "Memory Speed".to_string(),
                        value: value.trim().to_string(),
                    });
                }
                if let Some(value) = trimmed.strip_prefix("Type:") {
                    info.push(ParsedInfo {
                        label: "Memory Type".to_string(),
                        value: value.trim().to_string(),
                    });
                }
            }
        }

        Ok(SampleData {
            metrics: Vec::new(),
            info,
        })
    }
}

pub struct GpuInfoCollector;

impl GpuInfoCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for GpuInfoCollector {
    fn id(&self) -> &'static str {
        "gpu_info"
    }

    fn name(&self) -> &'static str {
        "GPU Info"
    }

    fn run_once(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let output = run_command("system_profiler", &["-detailLevel", "mini", "SPDisplaysDataType"])?;
        let mut info = Vec::new();
        let mut gpu_index = 0usize;

        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix("Chipset Model:") {
                gpu_index += 1;
                info.push(ParsedInfo {
                    label: format!("GPU {gpu_index} Model"),
                    value: value.trim().to_string(),
                });
            } else if let Some(value) = trimmed.strip_prefix("Vendor:") {
                if gpu_index > 0 {
                    info.push(ParsedInfo {
                        label: format!("GPU {gpu_index} Vendor"),
                        value: value.trim().to_string(),
                    });
                }
            } else if let Some(value) = trimmed.strip_prefix("VRAM (Total):") {
                if gpu_index > 0 {
                    info.push(ParsedInfo {
                        label: format!("GPU {gpu_index} VRAM"),
                        value: value.trim().to_string(),
                    });
                }
            } else if let Some(value) = trimmed.strip_prefix("Metal:") {
                if gpu_index > 0 {
                    info.push(ParsedInfo {
                        label: format!("GPU {gpu_index} Metal"),
                        value: value.trim().to_string(),
                    });
                }
            }
        }

        Ok(SampleData {
            metrics: Vec::new(),
            info,
        })
    }
}

pub struct OsVersionCollector;

impl OsVersionCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for OsVersionCollector {
    fn id(&self) -> &'static str {
        "os_version"
    }

    fn name(&self) -> &'static str {
        "macOS Version"
    }

    fn run_once(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut info = Vec::new();

        if let Ok(name) = run_command("sw_vers", &["-productName"]) {
            info.push(ParsedInfo {
                label: "macOS Name".to_string(),
                value: name,
            });
        }

        if let Ok(version) = run_command("sw_vers", &["-productVersion"]) {
            info.push(ParsedInfo {
                label: "macOS Version".to_string(),
                value: version,
            });
        }

        if let Ok(build) = run_command("sw_vers", &["-buildVersion"]) {
            info.push(ParsedInfo {
                label: "macOS Build".to_string(),
                value: build,
            });
        }

        Ok(SampleData {
            metrics: Vec::new(),
            info,
        })
    }
}

#[allow(deprecated)]
fn read_cpu_ticks() -> Result<Vec<[u64; 4]>, String> {
    let mut cpu_count: libc::natural_t = 0;
    let mut cpu_info: processor_info_array_t = std::ptr::null_mut();
    let mut cpu_info_count: mach_msg_type_number_t = 0;

    let kr = unsafe {
        host_processor_info(
            mach_host_self(),
            PROCESSOR_CPU_LOAD_INFO,
            &mut cpu_count,
            &mut cpu_info,
            &mut cpu_info_count,
        )
    };
    if kr != KERN_SUCCESS {
        return Err("host_processor_info failed".to_string());
    }

    let info_slice = unsafe { std::slice::from_raw_parts(cpu_info, cpu_info_count as usize) };
    let mut result = Vec::with_capacity(cpu_count as usize);

    for cpu in 0..cpu_count as usize {
        let base = cpu * libc::CPU_STATE_MAX as usize;
        let user = info_slice[base + libc::CPU_STATE_USER as usize] as u64;
        let system = info_slice[base + libc::CPU_STATE_SYSTEM as usize] as u64;
        let idle = info_slice[base + libc::CPU_STATE_IDLE as usize] as u64;
        let nice = info_slice[base + libc::CPU_STATE_NICE as usize] as u64;
        result.push([user, system, idle, nice]);
    }

    let size_bytes = cpu_info_count as usize * std::mem::size_of::<libc::integer_t>();
    unsafe {
        vm_deallocate(
            mach_task_self(),
            cpu_info as usize,
            size_bytes,
        );
    }

    Ok(result)
}

#[allow(deprecated)]
fn read_vm_stats() -> Result<vm_statistics64_data_t, String> {
    let mut stats: vm_statistics64_data_t = unsafe { std::mem::zeroed() };
    let mut count = (std::mem::size_of::<vm_statistics64_data_t>()
        / std::mem::size_of::<libc::integer_t>()) as mach_msg_type_number_t;

    let kr = unsafe {
        host_statistics64(
            mach_host_self(),
            HOST_VM_INFO64,
            &mut stats as *mut _ as *mut libc::integer_t,
            &mut count,
        )
    };

    if kr != KERN_SUCCESS {
        return Err("host_statistics64 failed".to_string());
    }

    Ok(stats)
}

fn read_swap_usage() -> Result<xsw_usage, String> {
    let mut usage: xsw_usage = unsafe { std::mem::zeroed() };
    let mut size = std::mem::size_of::<xsw_usage>();
    let cname = CString::new("vm.swapusage").map_err(|_| "invalid sysctl name".to_string())?;

    let rc = unsafe {
        libc::sysctlbyname(
            cname.as_ptr(),
            &mut usage as *mut _ as *mut c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err("sysctl vm.swapusage failed".to_string());
    }

    Ok(usage)
}
