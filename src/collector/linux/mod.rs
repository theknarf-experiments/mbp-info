mod cpu_info;
mod cpu_usage;
mod disk_usage;
mod load_average;
mod memory_info;
mod memory_usage;
mod os_version;
mod power;
mod temperature;
mod uptime;

use crate::data::CollectorRegistration;

use cpu_info::CpuInfoCollector;
use cpu_usage::CpuUsageCollector;
use disk_usage::DiskUsageCollector;
use load_average::LoadAverageCollector;
use memory_info::MemoryInfoCollector;
use memory_usage::MemoryUsageCollector;
use os_version::OsVersionCollector;
use power::PowerCollector;
use temperature::TemperatureCollector;
use uptime::UptimeCollector;

pub fn collectors(_use_sudo: bool) -> Vec<CollectorRegistration> {
    vec![
        CollectorRegistration::new(Box::new(|| Box::new(CpuInfoCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(CpuUsageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(MemoryInfoCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(MemoryUsageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(LoadAverageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(DiskUsageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(UptimeCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(TemperatureCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(PowerCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(OsVersionCollector::new()))),
    ]
}
