use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

use crate::collector::{run_command, Collector};

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
        let mut entries = Vec::new();
        let mut gpu_index = 0usize;

        for line in output.lines() {
            let trimmed = line.trim();
            if let Some(value) = trimmed.strip_prefix("Chipset Model:") {
                gpu_index += 1;
                let label = format!("GPU {gpu_index} Model");
                entries.push(ParsedDatum {
                    key: label.clone(),
                    label,
                    unit: String::new(),
                    retention: Retention::Latest,
                    value: SeriesValue::Text(value.trim().to_string()),
                });
            } else if let Some(value) = trimmed.strip_prefix("Vendor:") {
                if gpu_index > 0 {
                    let label = format!("GPU {gpu_index} Vendor");
                    entries.push(ParsedDatum {
                        key: label.clone(),
                        label,
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(value.trim().to_string()),
                    });
                }
            } else if let Some(value) = trimmed.strip_prefix("VRAM (Total):") {
                if gpu_index > 0 {
                    let label = format!("GPU {gpu_index} VRAM");
                    entries.push(ParsedDatum {
                        key: label.clone(),
                        label,
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(value.trim().to_string()),
                    });
                }
            } else if let Some(value) = trimmed.strip_prefix("Metal:") {
                if gpu_index > 0 {
                    let label = format!("GPU {gpu_index} Metal");
                    entries.push(ParsedDatum {
                        key: label.clone(),
                        label,
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
