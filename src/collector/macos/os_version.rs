use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

use crate::collector::{run_command, Collector};

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
        "macOS Version"
    }

    fn run_once(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut entries = Vec::new();

        if let Ok(name) = run_command("sw_vers", &["-productName"]) {
            entries.push(ParsedDatum {
                key: "macOS Name".to_string(),
                label: "macOS Name".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(name),
            });
        }

        if let Ok(version) = run_command("sw_vers", &["-productVersion"]) {
            entries.push(ParsedDatum {
                key: "macOS Version".to_string(),
                label: "macOS Version".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(version),
            });
        }

        if let Ok(build) = run_command("sw_vers", &["-buildVersion"]) {
            entries.push(ParsedDatum {
                key: "macOS Build".to_string(),
                label: "macOS Build".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(build),
            });
        }

        Ok(SampleData { entries })
    }
}
