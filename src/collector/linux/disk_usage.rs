use crate::collector::run_command;
use crate::collector::Collector;
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};

pub struct DiskUsageCollector;

impl DiskUsageCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for DiskUsageCollector {
    fn id(&self) -> &'static str {
        "disk_usage"
    }

    fn name(&self) -> &'static str {
        "Disk Usage"
    }

    fn run_once(&self) -> bool {
        true
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut entries = Vec::new();

        // Use df command to get disk usage information
        if let Ok(df_output) = run_command(
            "df",
            &["-h", "--output=source,fstype,size,used,avail,pcent,target"],
        ) {
            let lines: Vec<&str> = df_output.lines().collect();

            for (index, line) in lines.iter().enumerate() {
                // Skip header line
                if index == 0 {
                    continue;
                }

                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 6 {
                    let source = parts[0];
                    let fstype = parts[1];
                    let size = parts[2];
                    let used = parts[3];
                    let avail = parts[4];
                    let usage_percent = parts[5].trim_end_matches('%');
                    let target = parts[6];

                    // Skip filesystems we don't care about
                    if source.starts_with("tmpfs")
                        || source.starts_with("devtmpfs")
                        || fstype == "tmpfs"
                        || fstype == "devtmpfs"
                    {
                        continue;
                    }

                    let filesystem_name = if source.starts_with("/dev/") {
                        // Extract device name
                        source.split('/').last().unwrap_or(source)
                    } else {
                        source
                    };

                    // Only show real filesystems with usage
                    if let Ok(percent) = usage_percent.parse::<f64>() {
                        entries.push(ParsedDatum {
                            key: format!("{} Disk Usage|%", filesystem_name),
                            label: format!("{} Usage", filesystem_name),
                            unit: "%".to_string(),
                            retention: Retention::Latest,
                            value: SeriesValue::Number(percent),
                        });
                    }

                    entries.push(ParsedDatum {
                        key: format!("{} Disk Size", filesystem_name),
                        label: format!("{} Size", filesystem_name),
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(size.to_string()),
                    });

                    entries.push(ParsedDatum {
                        key: format!("{} Disk Used", filesystem_name),
                        label: format!("{} Used", filesystem_name),
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(used.to_string()),
                    });

                    entries.push(ParsedDatum {
                        key: format!("{} Disk Available", filesystem_name),
                        label: format!("{} Available", filesystem_name),
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(avail.to_string()),
                    });

                    entries.push(ParsedDatum {
                        key: format!("{} Mount Point", filesystem_name),
                        label: format!("{} Mount", filesystem_name),
                        unit: String::new(),
                        retention: Retention::Latest,
                        value: SeriesValue::Text(target.to_string()),
                    });
                }
            }
        }

        Ok(SampleData { entries })
    }
}
