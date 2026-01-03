use crate::collector::Collector;
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};
use std::fs;

pub struct CpuInfoCollector;

impl CpuInfoCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for CpuInfoCollector {
    fn id(&self) -> &'static str {
        "cpu_info"
    }

    fn name(&self) -> &'static str {
        "CPU Info"
    }

    fn run_once(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut entries = Vec::new();

        // Read /proc/cpuinfo for CPU information
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
            // Parse model name
            for line in cpuinfo.lines() {
                if line.starts_with("model name") {
                    if let Some(model) = line.split(':').nth(1) {
                        entries.push(ParsedDatum {
                            key: "CPU Brand".to_string(),
                            label: "CPU Brand".to_string(),
                            unit: String::new(),
                            retention: Retention::Latest,
                            value: SeriesValue::Text(model.trim().to_string()),
                        });
                        break;
                    }
                }
            }

            // Count processors
            let processor_count = cpuinfo
                .lines()
                .filter(|line| line.starts_with("processor"))
                .count();

            if processor_count > 0 {
                entries.push(ParsedDatum {
                    key: "CPU Cores (logical)".to_string(),
                    label: "CPU Cores (logical)".to_string(),
                    unit: String::new(),
                    retention: Retention::Latest,
                    value: SeriesValue::Text(processor_count.to_string()),
                });
            }
        }

        // Add architecture
        entries.push(ParsedDatum {
            key: "CPU Architecture".to_string(),
            label: "CPU Architecture".to_string(),
            unit: String::new(),
            retention: Retention::Latest,
            value: SeriesValue::Text(std::env::consts::ARCH.to_string()),
        });

        // Try to get CPU frequency from /proc/cpuinfo
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
            for line in cpuinfo.lines() {
                if line.starts_with("cpu MHz") {
                    if let Some(freq_str) = line.split(':').nth(1) {
                        if let Ok(mhz) = freq_str.trim().parse::<f64>() {
                            let ghz = mhz / 1000.0;
                            entries.push(ParsedDatum {
                                key: "CPU Frequency".to_string(),
                                label: "CPU Frequency".to_string(),
                                unit: String::new(),
                                retention: Retention::Latest,
                                value: SeriesValue::Text(format!("{ghz:.2} GHz")),
                            });
                            break;
                        }
                    }
                }
            }
        }

        Ok(SampleData { entries })
    }
}
