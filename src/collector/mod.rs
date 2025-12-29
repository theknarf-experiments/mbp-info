use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Sender,
    Arc,
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
pub const MAX_SAMPLES: usize = 120;

pub trait Collector: Send {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn requires_sudo(&self) -> bool {
        false
    }
    fn interval(&self) -> Duration {
        SAMPLE_INTERVAL
    }
    fn run_once(&self) -> bool {
        false
    }
    fn sample(&mut self) -> Result<SampleData, String>;
}

pub enum CollectorMessage {
    Sample {
        collector: &'static str,
        data: SampleData,
    },
    Error {
        collector: &'static str,
        error: String,
    },
}

pub fn spawn_collector(
    mut collector: Box<dyn Collector>,
    tx: Sender<CollectorMessage>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    let id = collector.id();

    thread::spawn(move || {
        if collector.run_once() {
            if stop.load(Ordering::Relaxed) {
                match collector.sample() {
                    Ok(data) => {
                        let _ = tx.send(CollectorMessage::Sample {
                            collector: id,
                            data,
                        });
                    }
                    Err(error) => {
                        let _ = tx.send(CollectorMessage::Error {
                            collector: id,
                            error,
                        });
                    }
                }
            }
            return;
        }

        loop {
            if !stop.load(Ordering::Relaxed) {
                break;
            }

            match collector.sample() {
                Ok(data) => {
                    let _ = tx.send(CollectorMessage::Sample {
                        collector: id,
                        data,
                    });
                }
                Err(error) => {
                    let _ = tx.send(CollectorMessage::Error {
                        collector: id,
                        error,
                    });
                }
            }

            sleep_with_stop(collector.interval(), &stop);
        }
    })
}

fn sleep_with_stop(duration: Duration, stop: &AtomicBool) {
    let mut elapsed = Duration::from_millis(0);
    let step = Duration::from_millis(200);

    while elapsed < duration {
        if !stop.load(Ordering::Relaxed) {
            break;
        }
        let remaining = duration.saturating_sub(elapsed);
        let sleep_for = if remaining < step { remaining } else { step };
        thread::sleep(sleep_for);
        elapsed += sleep_for;
    }
}

pub fn needs_sudo() -> bool {
    let uid = unsafe { libc::geteuid() };
    uid != 0
}

pub(crate) fn parse_key_value_lines(text: &str) -> SampleData {
    let mut entries = Vec::new();

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

        if let Some((value, unit)) = parse_numeric_value(rest) {
            let key = format!("{}|{}", label, unit);

            entries.push(ParsedDatum {
                key,
                label: label.to_string(),
                unit,
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(value),
            });
        } else {
            entries.push(ParsedDatum {
                key: label.to_string(),
                label: label.to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text(rest.to_string()),
            });
        }
    }

    SampleData { entries }
}

fn parse_numeric_value(input: &str) -> Option<(f64, String)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut number = String::new();
    let mut seen_digit = false;
    let mut seen_dot = false;
    let mut seen_sign = false;
    let mut chars = trimmed.chars().peekable();

    while let Some(&ch) = chars.peek() {
        let is_sign = (ch == '-' || ch == '+') && !seen_sign && !seen_digit;
        let is_digit = ch.is_ascii_digit();
        let is_dot = ch == '.' && !seen_dot;

        if is_sign {
            seen_sign = true;
            number.push(ch);
            chars.next();
            continue;
        }

        if is_digit {
            seen_digit = true;
            number.push(ch);
            chars.next();
            continue;
        }

        if is_dot {
            seen_dot = true;
            number.push(ch);
            chars.next();
            continue;
        }

        break;
    }

    if !seen_digit {
        return None;
    }

    let value = number.parse::<f64>().ok()?;
    let unit_raw: String = chars.collect::<String>();
    let unit = unit_raw
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphabetic() || *ch == '/' || *ch == '%')
        .collect::<String>();

    Some((value, unit))
}

pub(crate) fn run_command(command: &str, args: &[&str]) -> Result<String, String> {
    let output = Command::new(command)
        .args(args)
        .output()
        .map_err(|error| format!("failed to run {command}: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("{command} exited with status {}", output.status)
        } else {
            stderr
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout.trim().to_string())
}

pub(crate) fn bytes_to_mb(value: u64) -> f64 {
    value as f64 / (1024.0 * 1024.0)
}

#[cfg(target_os = "macos")]
pub mod macos;
