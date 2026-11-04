use std::process::Command;

use crate::data::SampleData;

use crate::collector::{parse_key_value_lines, Collector};

/// Samplers we want, in order of preference. Which ones exist depends on the
/// hardware: `smc` is Intel-only, while Apple Silicon exposes per-subsystem
/// power samplers instead. Noisy per-process samplers (tasks, disk, ...) are
/// left out on purpose.
const PREFERRED_SAMPLERS: [&str; 6] = [
    "smc",
    "cpu_power",
    "gpu_power",
    "ane_power",
    "thermal",
    "battery",
];

pub struct PowermetricsCollector {
    use_sudo: bool,
    samplers: Option<String>,
}

impl PowermetricsCollector {
    pub fn new(use_sudo: bool) -> Self {
        Self {
            use_sudo,
            samplers: None,
        }
    }

    fn samplers(&mut self) -> &str {
        self.samplers.get_or_insert_with(|| {
            let available = Command::new("powermetrics")
                .arg("-h")
                .output()
                .map(|output| {
                    let text = format!(
                        "{}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    );
                    parse_supported_samplers(&text)
                })
                .unwrap_or_default();
            select_samplers(&available)
        })
    }
}

/// Extracts sampler names from the "samplers are supported by --samplers"
/// section of `powermetrics -h`.
fn parse_supported_samplers(help: &str) -> Vec<String> {
    let mut samplers = Vec::new();
    let mut in_section = false;

    for line in help.lines() {
        let trimmed = line.trim();
        if trimmed.contains("samplers are supported by --samplers") {
            in_section = true;
            continue;
        }
        if !in_section {
            continue;
        }
        if trimmed.is_empty() {
            if samplers.is_empty() {
                continue;
            }
            break;
        }
        if let Some(name) = trimmed.split_whitespace().next() {
            samplers.push(name.to_string());
        }
    }

    samplers
}

fn select_samplers(available: &[String]) -> String {
    let selected: Vec<&str> = PREFERRED_SAMPLERS
        .iter()
        .copied()
        .filter(|sampler| available.iter().any(|name| name == sampler))
        .collect();

    if !selected.is_empty() {
        return selected.join(",");
    }

    // Discovery failed; fall back to a guess based on the architecture.
    if cfg!(target_arch = "x86_64") {
        "smc".to_string()
    } else {
        "cpu_power,gpu_power".to_string()
    }
}

impl Collector for PowermetricsCollector {
    fn id(&self) -> &'static str {
        "powermetrics"
    }

    fn name(&self) -> &'static str {
        "Powermetrics"
    }

    fn requires_sudo(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let samplers = self.samplers().to_string();
        let mut command = if self.use_sudo {
            let mut command = Command::new("sudo");
            command.arg("-n").arg("powermetrics");
            command
        } else {
            Command::new("powermetrics")
        };

        let output = command
            .args(["--samplers", &samplers, "-n", "1", "-i", "1000"])
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

#[cfg(test)]
mod tests {
    use super::*;

    const APPLE_SILICON_HELP: &str = "\
The following samplers are supported by --samplers:

    tasks             per task cpu usage and wakeup stats
    battery           battery and backlight info
    cpu_power         cpu power and frequency info
    thermal           thermal pressure notifications
    gpu_power         gpu power and frequency info
    ane_power         dedicated rail ane power and frequency info

and the following sampler groups are supported by --samplers:

    all           tasks,battery,cpu_power,thermal,gpu_power,ane_power
";

    #[test]
    fn parses_sampler_section_only() {
        let samplers = parse_supported_samplers(APPLE_SILICON_HELP);
        assert_eq!(
            samplers,
            [
                "tasks",
                "battery",
                "cpu_power",
                "thermal",
                "gpu_power",
                "ane_power"
            ]
        );
    }

    #[test]
    fn selects_available_preferred_samplers() {
        let samplers = parse_supported_samplers(APPLE_SILICON_HELP);
        assert_eq!(
            select_samplers(&samplers),
            "cpu_power,gpu_power,ane_power,thermal,battery"
        );
    }

    #[test]
    fn selects_smc_when_available() {
        let available = vec!["smc".to_string(), "tasks".to_string()];
        assert_eq!(select_samplers(&available), "smc");
    }
}
