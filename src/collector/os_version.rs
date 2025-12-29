use crate::data::{ParsedInfo, SampleData};

use super::{run_command, Collector};

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
        let mut info = Vec::new();

        if let Ok(name) = run_command("sw_vers", &["-productName"]) {
            info.push(ParsedInfo {
                label: "macOS Name".to_string(),
                value: name,
            });
        }

        if let Ok(version) = run_command("sw_vers", &["-productVersion"]) {
            info.push(ParsedInfo {
                label: "macOS Version".to_string(),
                value: version,
            });
        }

        if let Ok(build) = run_command("sw_vers", &["-buildVersion"]) {
            info.push(ParsedInfo {
                label: "macOS Build".to_string(),
                value: build,
            });
        }

        Ok(SampleData {
            metrics: Vec::new(),
            info,
        })
    }
}
