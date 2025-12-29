use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

use crate::collector::{run_command, Collector, MAX_SAMPLES};

pub struct SwapUsageCollector;

impl SwapUsageCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for SwapUsageCollector {
    fn id(&self) -> &'static str {
        "swap_usage"
    }

    fn name(&self) -> &'static str {
        "Swap Usage"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let usage = read_swap_usage()?;

        let entries = vec![
            ParsedDatum {
                key: "Swap Used|MB".to_string(),
                label: "Swap Used".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(usage.used_mb),
            },
            ParsedDatum {
                key: "Swap Free|MB".to_string(),
                label: "Swap Free".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(usage.free_mb),
            },
            ParsedDatum {
                key: "Swap Total|MB".to_string(),
                label: "Swap Total".to_string(),
                unit: "MB".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(usage.total_mb),
            },
        ];

        Ok(SampleData { entries })
    }
}

struct SwapUsage {
    total_mb: f64,
    used_mb: f64,
    free_mb: f64,
}

fn read_swap_usage() -> Result<SwapUsage, String> {
    let output = run_command("sysctl", &["vm.swapusage"])?;
    parse_swap_usage(&output)
}

fn parse_swap_usage(text: &str) -> Result<SwapUsage, String> {
    let mut total = None;
    let mut used = None;
    let mut free = None;

    let tokens: Vec<&str> = text.split_whitespace().collect();
    let mut index = 0;
    while index + 2 < tokens.len() {
        let key = tokens[index];
        let value = tokens[index + 2];
        if tokens[index + 1] == "=" {
            match key {
                "total" => total = Some(parse_swap_value(value)?),
                "used" => used = Some(parse_swap_value(value)?),
                "free" => free = Some(parse_swap_value(value)?),
                _ => {}
            }
            index += 3;
            continue;
        }
        index += 1;
    }

    let total = total.ok_or_else(|| "swap total missing".to_string())?;
    let used = used.ok_or_else(|| "swap used missing".to_string())?;
    let free = free.ok_or_else(|| "swap free missing".to_string())?;

    Ok(SwapUsage {
        total_mb: total,
        used_mb: used,
        free_mb: free,
    })
}

fn parse_swap_value(token: &str) -> Result<f64, String> {
    let trimmed = token.trim_matches(',');
    let mut number = String::new();
    let mut unit = String::new();

    for ch in trimmed.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            number.push(ch);
        } else if !ch.is_whitespace() {
            unit.push(ch);
        }
    }

    let value: f64 = number
        .parse()
        .map_err(|_| format!("invalid swap value: {token}"))?;
    let unit = unit.to_ascii_uppercase();

    let mb = match unit.as_str() {
        "K" | "KB" => value / 1024.0,
        "M" | "MB" | "" => value,
        "G" | "GB" => value * 1024.0,
        "T" | "TB" => value * 1024.0 * 1024.0,
        _ => return Err(format!("unknown swap unit: {unit}")),
    };

    if mb.is_finite() {
        Ok(mb)
    } else {
        Err("swap value not finite".to_string())
    }
}
