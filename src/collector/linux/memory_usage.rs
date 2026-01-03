use crate::collector::{Collector, MAX_SAMPLES};
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};
use std::fs;

pub struct MemoryUsageCollector;

impl MemoryUsageCollector {
    pub fn new() -> Self {
        Self
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
        let mut entries = Vec::new();

        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            let mut mem_total = 0u64;
            let mut mem_free = 0u64;
            let mut mem_available = 0u64;
            let mut mem_buffers = 0u64;
            let mut mem_cached = 0u64;
            let mut swap_total = 0u64;
            let mut swap_free = 0u64;

            for line in meminfo.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    let value = parts[1].parse::<u64>().unwrap_or(0);
                    match parts[0] {
                        "MemTotal:" => mem_total = value,
                        "MemFree:" => mem_free = value,
                        "MemAvailable:" => mem_available = value,
                        "Buffers:" => mem_buffers = value,
                        "Cached:" => mem_cached = value,
                        "SwapTotal:" => swap_total = value,
                        "SwapFree:" => swap_free = value,
                        _ => {}
                    }
                }
            }

            if mem_total > 0 {
                // Calculate used memory in different ways
                let used_kb = mem_total - mem_free;
                let used_active_kb = mem_total - mem_available;
                let cached_buffers_kb = mem_buffers + mem_cached;

                // Convert to MB for display
                let total_mb = mem_total as f64 / 1024.0;
                let used_mb = used_kb as f64 / 1024.0;
                let free_mb = mem_free as f64 / 1024.0;
                let available_mb = mem_available as f64 / 1024.0;
                let active_mb = used_active_kb as f64 / 1024.0;
                let cached_buffers_mb = cached_buffers_kb as f64 / 1024.0;

                // Memory usage percentages
                let used_percent = (used_kb as f64 / mem_total as f64) * 100.0;
                let active_percent = (used_active_kb as f64 / mem_total as f64) * 100.0;

                entries.push(ParsedDatum {
                    key: "Memory Total|MB".to_string(),
                    label: "Memory Total".to_string(),
                    unit: "MB".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(total_mb),
                });

                entries.push(ParsedDatum {
                    key: "Memory Used|MB".to_string(),
                    label: "Memory Used".to_string(),
                    unit: "MB".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(used_mb),
                });

                entries.push(ParsedDatum {
                    key: "Memory Free|MB".to_string(),
                    label: "Memory Free".to_string(),
                    unit: "MB".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(free_mb),
                });

                entries.push(ParsedDatum {
                    key: "Memory Available|MB".to_string(),
                    label: "Memory Available".to_string(),
                    unit: "MB".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(available_mb),
                });

                entries.push(ParsedDatum {
                    key: " Memory Active|MB".to_string(),
                    label: "Memory Active".to_string(),
                    unit: "MB".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(active_mb),
                });

                entries.push(ParsedDatum {
                    key: "Memory Cache+Buffers|MB".to_string(),
                    label: "Memory Cache+Buffers".to_string(),
                    unit: "MB".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(cached_buffers_mb),
                });

                entries.push(ParsedDatum {
                    key: "Memory Usage|%".to_string(),
                    label: "Memory Usage".to_string(),
                    unit: "%".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(used_percent),
                });

                entries.push(ParsedDatum {
                    key: "Memory Active|%".to_string(),
                    label: "Memory Active".to_string(),
                    unit: "%".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(active_percent),
                });
            }

            // Swap information
            if swap_total > 0 {
                let swap_used = swap_total - swap_free;
                let swap_total_mb = swap_total as f64 / 1024.0;
                let swap_used_mb = swap_used as f64 / 1024.0;
                let swap_free_mb = swap_free as f64 / 1024.0;
                let swap_used_percent = (swap_used as f64 / swap_total as f64) * 100.0;

                entries.push(ParsedDatum {
                    key: "Swap Total|MB".to_string(),
                    label: "Swap Total".to_string(),
                    unit: "MB".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(swap_total_mb),
                });

                entries.push(ParsedDatum {
                    key: "Swap Used|MB".to_string(),
                    label: "Swap Used".to_string(),
                    unit: "MB".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(swap_used_mb),
                });

                entries.push(ParsedDatum {
                    key: "Swap Free|MB".to_string(),
                    label: "Swap Free".to_string(),
                    unit: "MB".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(swap_free_mb),
                });

                entries.push(ParsedDatum {
                    key: "Swap Usage|%".to_string(),
                    label: "Swap Usage".to_string(),
                    unit: "%".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(swap_used_percent),
                });
            }
        }

        Ok(SampleData { entries })
    }
}
