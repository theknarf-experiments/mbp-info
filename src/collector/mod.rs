use std::ffi::CString;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use libc::c_void;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
pub const MAX_SAMPLES: usize = 120;

static NUMERIC_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(-?\d+(?:\.\d+)?)(?:\s*([A-Za-z/%]+))?\s*$").expect("valid regex")
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
        collector: &'static str,
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
                        let _ = tx.send(CollectorMessage::Sample {
                            collector: id,
                            data,
                        });
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
                    let _ = tx.send(CollectorMessage::Sample {
                        collector: id,
                        data,
                    });
                }
                Err(error) => {
                    let _ = tx.send(CollectorMessage::Error {
                        collector: id,
                        error,
                    });
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

pub(crate) fn parse_key_value_lines(text: &str) -> SampleData {
    let mut entries = Vec::new();

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
            let unit = caps
                .get(2)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let key = format!("{}|{}", label, unit);

            entries.push(ParsedDatum {
                key,
                label: label.to_string(),
                unit,
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(value),
            });
        } else {
            entries.push(ParsedDatum {
                key: label.to_string(),
                label: label.to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(rest.to_string()),
            });
        }
    }

    SampleData { entries }
}

pub(crate) fn run_command(command: &str, args: &[&str]) -> Result<String, String> {
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

pub(crate) fn sysctl_string(name: &str) -> Result<String, String> {
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

pub(crate) fn sysctl_u64(name: &str) -> Result<u64, String> {
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

pub(crate) fn sysctl_timeval(name: &str) -> Result<libc::timeval, String> {
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

pub(crate) fn bytes_to_mb(value: u64) -> f64 {
    value as f64 / (1024.0 * 1024.0)
}

mod cpu_info;
mod cpu_usage;
mod gpu_info;
mod memory_info;
mod memory_usage;
mod os_version;
mod powermetrics;
mod swap_usage;
mod uptime;

pub use cpu_info::CpuInfoCollector;
pub use cpu_usage::CpuUsageCollector;
pub use gpu_info::GpuInfoCollector;
pub use memory_info::MemoryInfoCollector;
pub use memory_usage::MemoryUsageCollector;
pub use os_version::OsVersionCollector;
pub use powermetrics::PowermetricsCollector;
pub use swap_usage::SwapUsageCollector;
pub use uptime::UptimeCollector;
