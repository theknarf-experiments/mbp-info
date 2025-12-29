use std::process::Command;

use crate::data::SampleData;

use super::{parse_key_value_lines, Collector};

pub struct PowermetricsCollector {
    use_sudo: bool,
}

impl PowermetricsCollector {
    pub fn new(use_sudo: bool) -> Self {
        Self { use_sudo }
    }
}

impl Collector for PowermetricsCollector {
    fn id(&self) -> &'static str {
        "powermetrics"
    }

    fn name(&self) -> &'static str {
        "Powermetrics (SMC)"
    }

    fn requires_sudo(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut command = if self.use_sudo {
            let mut command = Command::new("sudo");
            command.arg("-n").arg("powermetrics");
            command
        } else {
            Command::new("powermetrics")
        };

        let output = command
            .args(["--samplers", "smc", "-n", "1", "-i", "1000"])
            .output()
            .map_err(|error| format!("failed to run powermetrics: {error}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let message = if stderr.is_empty() {
                format!("powermetrics exited with status {}", output.status)
            } else if self.use_sudo && stderr.to_lowercase().contains("password") {
                "sudo authentication required. Use :reauth powermetrics".to_string()
            } else {
                stderr
            };
            return Err(message);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}{stderr}");
        let filtered = combined
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start().trim_start_matches('*').trim_start();
                !trimmed.starts_with("Sampled system activity")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let data = parse_key_value_lines(&filtered);

        if data.entries.is_empty() {
            Err("no metrics parsed from powermetrics output".to_string())
        } else {
            Ok(data)
        }
    }
}
