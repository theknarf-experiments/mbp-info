use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

use super::{sysctl_string, sysctl_u64, Collector};

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

        if let Ok(model) = sysctl_string("machdep.cpu.brand_string") {
            entries.push(ParsedDatum {
                key: "CPU Brand".to_string(),
                label: "CPU Brand".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(model),
            });
        }

        entries.push(ParsedDatum {
            key: "CPU Architecture".to_string(),
            label: "CPU Architecture".to_string(),
            unit: String::new(),
            retention: Retention::Latest,
            value: SeriesValue::Text(std::env::consts::ARCH.to_string()),
        });

        if let Ok(logical) = sysctl_u64("hw.logicalcpu") {
            entries.push(ParsedDatum {
                key: "CPU Cores (logical)".to_string(),
                label: "CPU Cores (logical)".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(logical.to_string()),
            });
        }

        if let Ok(physical) = sysctl_u64("hw.physicalcpu") {
            entries.push(ParsedDatum {
                key: "CPU Cores (physical)".to_string(),
                label: "CPU Cores (physical)".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(physical.to_string()),
            });
        }

        if let Ok(freq) = sysctl_u64("hw.cpufrequency") {
            let ghz = freq as f64 / 1_000_000_000.0;
            entries.push(ParsedDatum {
                key: "CPU Frequency".to_string(),
                label: "CPU Frequency".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(format!("{ghz:.2} GHz")),
            });
        }

        if let Ok(model) = sysctl_string("hw.model") {
            entries.push(ParsedDatum {
                key: "Machine Model".to_string(),
                label: "Machine Model".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(model),
            });
        }

        Ok(SampleData { entries })
    }
}
