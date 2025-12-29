use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

use super::sysctl::sysctl_timeval;
use crate::collector::{Collector, MAX_SAMPLES};

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
        let boot_time = sysctl_timeval("kern.boottime")?;
        let boot = Duration::from_secs(boot_time.tv_sec as u64)
            + Duration::from_micros(boot_time.tv_usec as u64);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("time error: {error}"))?;
        let uptime = now.saturating_sub(boot);

        Ok(SampleData {
            entries: vec![ParsedDatum {
                key: "Uptime|s".to_string(),
                label: "Uptime".to_string(),
                unit: "s".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(uptime.as_secs_f64()),
            }],
        })
    }
}
