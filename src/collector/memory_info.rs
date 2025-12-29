use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

use super::macos::sysctl_u64;
use super::{run_command, Collector};

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

        if let Ok(total) = sysctl_u64("hw.memsize") {
            let gb = total as f64 / (1024.0 * 1024.0 * 1024.0);
            entries.push(ParsedDatum {
                key: "Memory Total".to_string(),
                label: "Memory Total".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(format!("{gb:.2} GB")),
            });
        }

        if let Ok(output) = run_command(
            "system_profiler",
            &["-detailLevel", "mini", "SPMemoryDataType"],
        ) {
            for line in output.lines() {
                let trimmed = line.trim();
                if let Some(value) = trimmed.strip_prefix("Speed:") {
                    entries.push(ParsedDatum {
                        key: "Memory Speed".to_string(),
                        label: "Memory Speed".to_string(),
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(value.trim().to_string()),
                    });
                }
                if let Some(value) = trimmed.strip_prefix("Type:") {
                    entries.push(ParsedDatum {
                        key: "Memory Type".to_string(),
                        label: "Memory Type".to_string(),
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(value.trim().to_string()),
                    });
                }
            }
        }

        Ok(SampleData { entries })
    }
}
