mod cpu_info;
mod cpu_usage;
mod gpu_info;
mod memory_info;
mod memory_usage;
mod os_version;
mod powermetrics;
mod swap_usage;
mod sysctl;
mod uptime;

use crate::data::CollectorRegistration;

use cpu_info::CpuInfoCollector;
use cpu_usage::CpuUsageCollector;
use gpu_info::GpuInfoCollector;
use memory_info::MemoryInfoCollector;
use memory_usage::MemoryUsageCollector;
use os_version::OsVersionCollector;
use powermetrics::PowermetricsCollector;
use swap_usage::SwapUsageCollector;
use uptime::UptimeCollector;

pub fn collectors(use_sudo: bool) -> Vec<CollectorRegistration> {
    vec![
        CollectorRegistration::new(Box::new(move || {
            Box::new(PowermetricsCollector::new(use_sudo))
        })),
        CollectorRegistration::new(Box::new(|| Box::new(CpuUsageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(MemoryUsageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(SwapUsageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(UptimeCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(CpuInfoCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(MemoryInfoCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(GpuInfoCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(OsVersionCollector::new()))),
    ]
}
