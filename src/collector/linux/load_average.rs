use crate::collector::{Collector, MAX_SAMPLES};
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};
use std::fs;

pub struct LoadAverageCollector;

impl LoadAverageCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for LoadAverageCollector {
    fn id(&self) -> &'static str {
        "load_average"
    }

    fn name(&self) -> &'static str {
        "Load Average"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut entries = Vec::new();

        if let Ok(loadavg) = fs::read_to_string("/proc/loadavg") {
            let parts: Vec<&str> = loadavg.trim().split_whitespace().collect();
            if parts.len() >= 3 {
                // Parse load averages (1min, 5min, 15min)
                if let (Ok(load1), Ok(load5), Ok(load15)) = (
                    parts[0].parse::<f64>(),
                    parts[1].parse::<f64>(),
                    parts[2].parse::<f64>(),
                ) {
                    entries.push(ParsedDatum {
                        key: "Load Average 1min".to_string(),
                        label: "Load Average 1min".to_string(),
                        unit: String::new(),
                        retention: Retention::Rolling(MAX_SAMPLES),
                        value: SeriesValue::Number(load1),
                    });

                    entries.push(ParsedDatum {
                        key: "Load Average 5min".to_string(),
                        label: "Load Average 5min".to_string(),
                        unit: String::new(),
                        retention: Retention::Rolling(MAX_SAMPLES),
                        value: SeriesValue::Number(load5),
                    });

                    entries.push(ParsedDatum {
                        key: "Load Average 15min".to_string(),
                        label: "Load Average 15min".to_string(),
                        unit: String::new(),
                        retention: Retention::Rolling(MAX_SAMPLES),
                        value: SeriesValue::Number(load15),
                    });

                    // Also extract running/total processes if available
                    if parts.len() >= 5 {
                        let running_total = parts[3]; // format: "running/total"
                        if let Some(slash_pos) = running_total.find('/') {
                            let running_str = &running_total[..slash_pos];
                            let total_str = &running_total[slash_pos + 1..];

                            if let (Ok(running), Ok(total)) =
                                (running_str.parse::<u64>(), total_str.parse::<u64>())
                            {
                                entries.push(ParsedDatum {
                                    key: "Running Processes".to_string(),
                                    label: "Running Processes".to_string(),
                                    unit: String::new(),
                                    retention: Retention::Rolling(MAX_SAMPLES),
                                    value: SeriesValue::Number(running as f64),
                                });

                                entries.push(ParsedDatum {
                                    key: "Total Processes".to_string(),
                                    label: "Total Processes".to_string(),
                                    unit: String::new(),
                                    retention: Retention::Rolling(MAX_SAMPLES),
                                    value: SeriesValue::Number(total as f64),
                                });
                            }
                        }
                    }
                }
            }
        }

        Ok(SampleData { entries })
    }
}
