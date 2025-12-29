use crate::data::{ParsedInfo, SampleData};

use super::{run_command, Collector};

pub struct GpuInfoCollector;

impl GpuInfoCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for GpuInfoCollector {
    fn id(&self) -> &'static str {
        "gpu_info"
    }

    fn name(&self) -> &'static str {
        "GPU Info"
    }

    fn run_once(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let output = run_command(
            "system_profiler",
            &["-detailLevel", "mini", "SPDisplaysDataType"],
        )?;
        let mut info = Vec::new();
        let mut gpu_index = 0usize;

        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix("Chipset Model:") {
                gpu_index += 1;
                info.push(ParsedInfo {
                    label: format!("GPU {gpu_index} Model"),
                    value: value.trim().to_string(),
                });
            } else if let Some(value) = trimmed.strip_prefix("Vendor:") {
                if gpu_index > 0 {
                    info.push(ParsedInfo {
                        label: format!("GPU {gpu_index} Vendor"),
                        value: value.trim().to_string(),
                    });
                }
            } else if let Some(value) = trimmed.strip_prefix("VRAM (Total):") {
                if gpu_index > 0 {
                    info.push(ParsedInfo {
                        label: format!("GPU {gpu_index} VRAM"),
                        value: value.trim().to_string(),
                    });
                }
            } else if let Some(value) = trimmed.strip_prefix("Metal:") {
                if gpu_index > 0 {
                    info.push(ParsedInfo {
                        label: format!("GPU {gpu_index} Metal"),
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
