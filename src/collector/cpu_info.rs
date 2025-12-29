use crate::data::{ParsedInfo, SampleData};

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
        let mut info = Vec::new();

        if let Ok(model) = sysctl_string("machdep.cpu.brand_string") {
            info.push(ParsedInfo {
                label: "CPU Brand".to_string(),
                value: model,
            });
        }

        info.push(ParsedInfo {
            label: "CPU Architecture".to_string(),
            value: std::env::consts::ARCH.to_string(),
        });

        if let Ok(logical) = sysctl_u64("hw.logicalcpu") {
            info.push(ParsedInfo {
                label: "CPU Cores (logical)".to_string(),
                value: logical.to_string(),
            });
        }

        if let Ok(physical) = sysctl_u64("hw.physicalcpu") {
            info.push(ParsedInfo {
                label: "CPU Cores (physical)".to_string(),
                value: physical.to_string(),
            });
        }

        if let Ok(freq) = sysctl_u64("hw.cpufrequency") {
            let ghz = freq as f64 / 1_000_000_000.0;
            info.push(ParsedInfo {
                label: "CPU Frequency".to_string(),
                value: format!("{ghz:.2} GHz"),
            });
        }

        if let Ok(model) = sysctl_string("hw.model") {
            info.push(ParsedInfo {
                label: "Machine Model".to_string(),
                value: model,
            });
        }

        Ok(SampleData {
            metrics: Vec::new(),
            info,
        })
    }
}
