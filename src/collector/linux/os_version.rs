use crate::collector::{run_command, Collector};
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};
use std::fs;

pub struct OsVersionCollector;

impl OsVersionCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for OsVersionCollector {
    fn id(&self) -> &'static str {
        "os_version"
    }

    fn name(&self) -> &'static str {
        "Linux Version"
    }

    fn run_once(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut entries = Vec::new();

        // Try to read from /etc/os-release for distribution info
        if let Ok(os_release) = fs::read_to_string("/etc/os-release") {
            let mut name = String::new();
            let mut version = String::new();
            let mut pretty_name = String::new();

            for line in os_release.lines() {
                if line.starts_with("NAME=") {
                    name = line
                        .split('=')
                        .nth(1)
                        .unwrap_or("")
                        .trim_matches('"')
                        .to_string();
                } else if line.starts_with("VERSION=") {
                    version = line
                        .split('=')
                        .nth(1)
                        .unwrap_or("")
                        .trim_matches('"')
                        .to_string();
                } else if line.starts_with("PRETTY_NAME=") {
                    pretty_name = line
                        .split('=')
                        .nth(1)
                        .unwrap_or("")
                        .trim_matches('"')
                        .to_string();
                }
            }

            if !pretty_name.is_empty() {
                entries.push(ParsedDatum {
                    key: "Linux Distribution".to_string(),
                    label: "Linux Distribution".to_string(),
                    unit: String::new(),
                    retention: Retention::Latest,
                    value: SeriesValue::Text(pretty_name),
                });
            } else if !name.is_empty() {
                entries.push(ParsedDatum {
                    key: "Linux Distribution".to_string(),
                    label: "Linux Distribution".to_string(),
                    unit: String::new(),
                    retention: Retention::Latest,
                    value: SeriesValue::Text(name),
                });
            }

            if !version.is_empty() {
                entries.push(ParsedDatum {
                    key: "Linux Version".to_string(),
                    label: "Linux Version".to_string(),
                    unit: String::new(),
                    retention: Retention::Latest,
                    value: SeriesValue::Text(version),
                });
            }
        }

        // Get kernel version from uname
        if let Ok(kernel_version) = run_command("uname", &["-r"]) {
            entries.push(ParsedDatum {
                key: "Kernel Version".to_string(),
                label: "Kernel Version".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(kernel_version),
            });
        }

        // Get kernel name from uname
        if let Ok(kernel_name) = run_command("uname", &["-s"]) {
            entries.push(ParsedDatum {
                key: "Kernel Name".to_string(),
                label: "Kernel Name".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(kernel_name),
            });
        }

        Ok(SampleData { entries })
    }
}
