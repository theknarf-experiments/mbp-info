use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue, MAX_SAMPLES};

use super::Collector;

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
                    entries: Vec::new(),
                });
            }
        };

        self.last_ticks = Some(current.clone());

        let mut entries = Vec::new();
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
            entries.push(ParsedDatum {
                key: format!("{label}|%"),
                label,
                unit: "%".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(usage),
            });
        }

        Ok(SampleData { entries })
    }
}

// libc marks these Mach APIs deprecated; keep them for now to avoid the extra mach2 dependency.
#[allow(deprecated)]
fn read_cpu_ticks() -> Result<Vec<[u64; 4]>, String> {
    use libc::{
        host_processor_info, mach_host_self, mach_msg_type_number_t, mach_task_self,
        processor_info_array_t, vm_deallocate, KERN_SUCCESS, PROCESSOR_CPU_LOAD_INFO,
    };

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
        vm_deallocate(mach_task_self(), cpu_info as usize, size_bytes);
    }

    Ok(result)
}
