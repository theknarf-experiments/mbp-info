use crate::collector::Collector;
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

pub struct UptimeCollector;

impl UptimeCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for UptimeCollector {
    fn id(&self) -> &'static str {
        "uptime"
    }

    fn name(&self) -> &'static str {
        "Uptime"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut entries = Vec::new();

        // Read from /proc/uptime
        if let Ok(uptime_content) = std::fs::read_to_string("/proc/uptime") {
            let parts: Vec<&str> = uptime_content.trim().split_whitespace().collect();
            if let Some(uptime_seconds_str) = parts.first() {
                if let Ok(uptime_seconds) = uptime_seconds_str.parse::<f64>() {
                    let uptime_days = (uptime_seconds / 86400.0).floor();
                    let uptime_hours = ((uptime_seconds % 86400.0) / 3600.0).floor();
                    let uptime_minutes = ((uptime_seconds % 3600.0) / 60.0).floor();
                    let uptime_seconds_only = (uptime_seconds % 60.0).floor();

                    entries.push(ParsedDatum {
                        key: "Uptime|seconds".to_string(),
                        label: "Uptime (seconds)".to_string(),
                        unit: "s".to_string(),
                        retention: Retention::Rolling(120),
                        value: SeriesValue::Number(uptime_seconds),
                    });

                    entries.push(ParsedDatum {
                        key: "Uptime|days".to_string(),
                        label: "Uptime (days)".to_string(),
                        unit: "d".to_string(),
                        retention: Retention::Latest,
                        value: SeriesValue::Number(uptime_days),
                    });

                    entries.push(ParsedDatum {
                        key: "Uptime|formatted".to_string(),
                        label: "Uptime".to_string(),
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(format!(
                            "{}d {}h {}m {}s",
                            uptime_days, uptime_hours, uptime_minutes, uptime_seconds_only
                        )),
                    });

                    // Get boot time if possible
                    let boot_timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs() as f64
                        - uptime_seconds;

                    entries.push(ParsedDatum {
                        key: "Boot Time".to_string(),
                        label: "Boot Time".to_string(),
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(
                            std::process::Command::new("date")
                                .arg("-d")
                                .arg(&format!("@{}", boot_timestamp as u64))
                                .output()
                                .ok()
                                .and_then(|o| String::from_utf8(o.stdout).ok())
                                .map(|s| s.trim().to_string())
                                .unwrap_or_else(|| "Unknown".to_string()),
                        ),
                    });
                }
            }
        }

        Ok(SampleData { entries })
    }
}
