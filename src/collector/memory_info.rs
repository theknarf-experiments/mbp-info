use crate::data::{ParsedInfo, SampleData};

use super::{run_command, sysctl_u64, Collector};

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

        if let Ok(output) = run_command(
            "system_profiler",
            &["-detailLevel", "mini", "SPMemoryDataType"],
        ) {
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
