use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue, MAX_SAMPLES};

use super::{bytes_to_mb, Collector};

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
        let free = (vm_stat.free_count + vm_stat.speculative_count) as u64 * self.page_size;
        let used = active + inactive + wired + compressed;

        let entries = vec![
            ParsedDatum {
                key: "Memory Used|MB".to_string(),
                label: "Memory Used".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(bytes_to_mb(used)),
            },
            ParsedDatum {
                key: "Memory Free|MB".to_string(),
                label: "Memory Free".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(bytes_to_mb(free)),
            },
            ParsedDatum {
                key: "Memory Wired|MB".to_string(),
                label: "Memory Wired".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(bytes_to_mb(wired)),
            },
            ParsedDatum {
                key: "Memory Compressed|MB".to_string(),
                label: "Memory Compressed".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(bytes_to_mb(compressed)),
            },
        ];

        Ok(SampleData { entries })
    }
}

// libc marks these Mach APIs deprecated; keep them for now to avoid the extra mach2 dependency.
#[allow(deprecated)]
fn read_vm_stats() -> Result<libc::vm_statistics64_data_t, String> {
    use libc::{
        host_statistics64, mach_host_self, mach_msg_type_number_t, HOST_VM_INFO64, KERN_SUCCESS,
    };

    let mut stats: libc::vm_statistics64_data_t = unsafe { std::mem::zeroed() };
    let mut count = (std::mem::size_of::<libc::vm_statistics64_data_t>()
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
