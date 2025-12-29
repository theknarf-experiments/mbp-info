use std::process::Command;
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use libc::geteuid;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::data::{ParsedInfo, ParsedMetric, SampleData};

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

static NUMERIC_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(-?\d+(?:\.\d+)?)(?:\s*([A-Za-z/%]+))?\s*$")
        .expect("valid regex")
});

pub enum CollectorMessage {
    Sample(SampleData),
    Error(String),
}

pub fn spawn_powermetrics(tx: Sender<CollectorMessage>, use_sudo: bool) {
    thread::spawn(move || loop {
        let result = sample_powermetrics(use_sudo);
        let _ = match result {
            Ok(data) => tx.send(CollectorMessage::Sample(data)),
            Err(error) => tx.send(CollectorMessage::Error(error)),
        };
        thread::sleep(SAMPLE_INTERVAL);
    });
}

fn parse_powermetrics(text: &str) -> SampleData {
    let mut metrics = Vec::new();
    let mut info = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains(':') {
            continue;
        }

        let mut parts = trimmed.splitn(2, ':');
        let label = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();
        if label.is_empty() || rest.is_empty() {
            continue;
        }

        if let Some(caps) = NUMERIC_RE.captures(rest) {
            let value: f64 = match caps.get(1).map(|m| m.as_str().parse()) {
                Some(Ok(value)) => value,
                _ => continue,
            };
            let unit = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
            let key = format!("{}|{}", label, unit);

            metrics.push(ParsedMetric {
                key,
                label: label.to_string(),
                unit,
                value,
            });
        } else {
            info.push(ParsedInfo {
                label: label.to_string(),
                value: rest.to_string(),
            });
        }
    }

    SampleData { metrics, info }
}

fn sample_powermetrics(use_sudo: bool) -> Result<SampleData, String> {
    let mut command = if use_sudo {
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
        } else if use_sudo && stderr.to_lowercase().contains("password") {
            "sudo authentication required. Press 'a' to authenticate.".to_string()
        } else {
            stderr
        };
        return Err(message);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    let data = parse_powermetrics(&combined);

    if data.metrics.is_empty() && data.info.is_empty() {
        Err("no metrics parsed from powermetrics output".to_string())
    } else {
        Ok(data)
    }
}

pub fn needs_sudo() -> bool {
    let uid = unsafe { geteuid() };
    uid != 0
}
