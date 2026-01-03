use crate::collector::Collector;
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};
use std::fs;

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
        let mut entries = Vec::new();

        // Read /proc/meminfo for memory information
        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            for line in meminfo.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    match parts[0] {
                        "MemTotal:" => {
                            if let Ok(total_kb) = parts[1].parse::<u64>() {
                                let total_gb = total_kb as f64 / (1024.0 * 1024.0);
                                entries.push(ParsedDatum {
                                    key: "Memory Total".to_string(),
                                    label: "Memory Total".to_string(),
                                    unit: String::new(),
                                    retention: Retention::Latest,
                                    value: SeriesValue::Text(format!("{total_gb:.2} GB")),
                                });
                            }
                        }
                        "MemAvailable:" => {
                            if let Ok(available_kb) = parts[1].parse::<u64>() {
                                let available_gb = available_kb as f64 / (1024.0 * 1024.0);
                                entries.push(ParsedDatum {
                                    key: "Memory Available".to_string(),
                                    label: "Memory Available".to_string(),
                                    unit: String::new(),
                                    retention: Retention::Latest,
                                    value: SeriesValue::Text(format!("{available_gb:.2} GB")),
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // Try to get memory type and speed from dmidecode (requires root)
        // For now, we'll add basic memory info that doesn't require root
        entries.push(ParsedDatum {
            key: "Memory Type".to_string(),
            label: "Memory Type".to_string(),
            unit: String::new(),
            retention: Retention::Latest,
            value: SeriesValue::Text("Unknown (requires dmidecode)".to_string()),
        });

        entries.push(ParsedDatum {
            key: "Memory Speed".to_string(),
            label: "Memory Speed".to_string(),
            unit: String::new(),
            retention: Retention::Latest,
            value: SeriesValue::Text("Unknown (requires dmidecode)".to_string()),
        });

        Ok(SampleData { entries })
    }
}
