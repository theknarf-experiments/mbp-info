use crate::collector::{Collector, MAX_SAMPLES};
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};
use std::fs;

pub struct CpuUsageCollector {
    last_ticks: Option<CpuTicks>,
}

#[derive(Debug, Clone)]
struct CpuTicks {
    total: u64,
    idle: u64,
    cores: Vec<CoreTicks>,
}

#[derive(Debug, Clone)]
struct CoreTicks {
    total: u64,
    idle: u64,
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

        // Overall CPU usage
        let total_delta = current.total.saturating_sub(previous.total);
        let idle_delta = current.idle.saturating_sub(previous.idle);

        if total_delta > 0 {
            let usage = ((total_delta - idle_delta) as f64 / total_delta as f64) * 100.0;
            entries.push(ParsedDatum {
                key: "CPU Total usage|%".to_string(),
                label: "CPU Total usage".to_string(),
                unit: "%".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(usage),
            });
        }

        // Per-core usage
        for (index, (now, prev)) in current.cores.iter().zip(previous.cores.iter()).enumerate() {
            let total_delta = now.total.saturating_sub(prev.total);
            let idle_delta = now.idle.saturating_sub(prev.idle);

            if total_delta > 0 {
                let usage = ((total_delta - idle_delta) as f64 / total_delta as f64) * 100.0;
                let label = format!("CPU Core {} usage", index);
                entries.push(ParsedDatum {
                    key: format!("{label}|%"),
                    label,
                    unit: "%".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(usage),
                });
            }
        }

        Ok(SampleData { entries })
    }
}

fn read_cpu_ticks() -> Result<CpuTicks, String> {
    let content = fs::read_to_string("/proc/stat")
        .map_err(|e| format!("Failed to read /proc/stat: {}", e))?;

    let mut total_ticks = None;
    let mut idle_ticks = None;
    let mut core_ticks = Vec::new();

    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "cpu" => {
                // Overall CPU stats
                let numbers: Result<Vec<u64>, _> = parts[1..].iter().map(|s| s.parse()).collect();
                match numbers {
                    Ok(nums) if nums.len() >= 4 => {
                        let user = nums[0];
                        let nice = nums[1];
                        let system = nums[2];
                        let idle = nums[3];
                        let total = user + nice + system + idle;
                        total_ticks = Some(total);
                        idle_ticks = Some(idle);
                    }
                    _ => return Err("Invalid CPU stats format".to_string()),
                }
            }
            core if core.starts_with("cpu") => {
                // Individual core stats
                let numbers: Result<Vec<u64>, _> = parts[1..].iter().map(|s| s.parse()).collect();
                match numbers {
                    Ok(nums) if nums.len() >= 4 => {
                        let user = nums[0];
                        let nice = nums[1];
                        let system = nums[2];
                        let idle = nums[3];
                        let total = user + nice + system + idle;
                        core_ticks.push(CoreTicks { total, idle });
                    }
                    _ => return Err("Invalid core stats format".to_string()),
                }
            }
            _ => {}
        }
    }

    match (total_ticks, idle_ticks) {
        (Some(total), Some(idle)) => Ok(CpuTicks {
            total,
            idle,
            cores: core_ticks,
        }),
        _ => Err("Could not parse CPU stats".to_string()),
    }
}
