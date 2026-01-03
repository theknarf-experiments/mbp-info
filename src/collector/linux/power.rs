use crate::collector::{Collector, MAX_SAMPLES};
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};
use std::fs;

pub struct PowerCollector;

impl PowerCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for PowerCollector {
    fn id(&self) -> &'static str {
        "power"
    }

    fn name(&self) -> &'static str {
        "Power & Battery"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut entries = Vec::new();

        // Read from /sys/class/power_supply/
        let power_supply_dir = "/sys/class/power_supply";
        if let Ok(entries_dir) = fs::read_dir(power_supply_dir) {
            for entry in entries_dir.flatten() {
                let path = entry.path();
                if let Some(device_name) = path.file_name().and_then(|n| n.to_str()) {
                    // Check device type
                    let type_path = path.join("type");
                    if let Ok(device_type) = fs::read_to_string(&type_path) {
                        let device_type = device_type.trim();

                        match device_type {
                            "Battery" => {
                                read_battery_info(&path, device_name, &mut entries)?;
                            }
                            "Mains" => {
                                read_mains_info(&path, device_name, &mut entries)?;
                            }
                            "USB" => {
                                read_usb_power_info(&path, device_name, &mut entries)?;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        // If no battery info found, add a note
        if entries.is_empty() {
            entries.push(ParsedDatum {
                key: "Power Status".to_string(),
                label: "Power Status".to_string(),
                unit: String::new(),
                retention: Retention::Latest,
                value: SeriesValue::Text("No power supply devices found".to_string()),
            });
        }

        Ok(SampleData { entries })
    }
}

fn read_battery_info(
    path: &std::path::Path,
    device_name: &str,
    entries: &mut Vec<ParsedDatum>,
) -> Result<(), String> {
    let mut battery_data = std::collections::HashMap::new();

    // Read common battery files
    let files_to_read = [
        ("status", "Status"),
        ("capacity", "Capacity"),
        ("capacity_level", "Capacity Level"),
        ("charge_now", "Charge Now"),
        ("charge_full", "Charge Full"),
        ("energy_now", "Energy Now"),
        ("energy_full", "Energy Full"),
        ("voltage_now", "Voltage"),
        ("current_now", "Current"),
        ("power_now", "Power"),
        ("health", "Health"),
        ("technology", "Technology"),
        ("model_name", "Model"),
        ("manufacturer", "Manufacturer"),
        ("serial_number", "Serial"),
    ];

    for (filename, key) in files_to_read.iter() {
        let file_path = path.join(filename);
        if let Ok(content) = fs::read_to_string(&file_path) {
            let value = content.trim().to_string();
            if !value.is_empty() {
                battery_data.insert(*key, value);
            }
        }
    }

    if battery_data.is_empty() {
        return Ok(());
    }

    let prefix = format!("Battery {}", device_name);

    // Status information
    if let Some(status) = battery_data.get("Status") {
        entries.push(ParsedDatum {
            key: format!("{} Status", prefix),
            label: format!("{} Status", prefix),
            unit: String::new(),
            retention: Retention::Rolling(MAX_SAMPLES),
            value: SeriesValue::Text(status.clone()),
        });
    }

    // Capacity (most common and important)
    if let Some(capacity_str) = battery_data.get("Capacity") {
        if let Ok(capacity) = capacity_str.parse::<f64>() {
            entries.push(ParsedDatum {
                key: format!("{} Capacity|%", prefix),
                label: format!("{} Capacity", prefix),
                unit: "%".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(capacity),
            });
        }
    }

    // Energy/Charge information
    if let (Some(energy_now), Some(energy_full)) = (
        battery_data
            .get("Energy Now")
            .and_then(|s| s.parse::<f64>().ok()),
        battery_data
            .get("Energy Full")
            .and_then(|s| s.parse::<f64>().ok()),
    ) {
        if energy_full > 0.0 {
            let energy_percent = (energy_now / energy_full) * 100.0;
            entries.push(ParsedDatum {
                key: format!("{} Energy|%", prefix),
                label: format!("{} Energy", prefix),
                unit: "%".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(energy_percent),
            });
        }
    }

    if let (Some(charge_now), Some(charge_full)) = (
        battery_data
            .get("Charge Now")
            .and_then(|s| s.parse::<f64>().ok()),
        battery_data
            .get("Charge Full")
            .and_then(|s| s.parse::<f64>().ok()),
    ) {
        if charge_full > 0.0 {
            let charge_percent = (charge_now / charge_full) * 100.0;
            entries.push(ParsedDatum {
                key: format!("{} Charge|%", prefix),
                label: format!("{} Charge", prefix),
                unit: "%".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(charge_percent),
            });
        }
    }

    // Power consumption (in microwatts usually)
    if let Some(power_str) = battery_data.get("Power") {
        if let Ok(power_uw) = power_str.parse::<f64>() {
            let power_mw = power_uw / 1000.0; // Convert to milliwatts
            entries.push(ParsedDatum {
                key: format!("{} Power|mW", prefix),
                label: format!("{} Power", prefix),
                unit: "mW".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(power_mw),
            });
        }
    }

    // Voltage (in microvolts usually)
    if let Some(voltage_str) = battery_data.get("Voltage") {
        if let Ok(voltage_uv) = voltage_str.parse::<f64>() {
            let voltage_mv = voltage_uv / 1000.0; // Convert to millivolts
            entries.push(ParsedDatum {
                key: format!("{} Voltage|mV", prefix),
                label: format!("{} Voltage", prefix),
                unit: "mV".to_string(),
                retention: Retention::Rolling(MAX_SAMPLES),
                value: SeriesValue::Number(voltage_mv),
            });
        }
    }

    // Static battery information
    if let Some(technology) = battery_data.get("Technology") {
        entries.push(ParsedDatum {
            key: format!("{} Technology", prefix),
            label: format!("{} Technology", prefix),
            unit: String::new(),
            retention: Retention::Latest,
            value: SeriesValue::Text(technology.clone()),
        });
    }

    if let Some(model) = battery_data.get("Model") {
        entries.push(ParsedDatum {
            key: format!("{} Model", prefix),
            label: format!("{} Model", prefix),
            unit: String::new(),
            retention: Retention::Latest,
            value: SeriesValue::Text(model.clone()),
        });
    }

    if let Some(manufacturer) = battery_data.get("Manufacturer") {
        entries.push(ParsedDatum {
            key: format!("{} Manufacturer", prefix),
            label: format!("{} Manufacturer", prefix),
            unit: String::new(),
            retention: Retention::Latest,
            value: SeriesValue::Text(manufacturer.clone()),
        });
    }

    if let Some(health_str) = battery_data.get("Health") {
        entries.push(ParsedDatum {
            key: format!("{} Health", prefix),
            label: format!("{} Health", prefix),
            unit: String::new(),
            retention: Retention::Latest,
            value: SeriesValue::Text(health_str.clone()),
        });
    }

    Ok(())
}

fn read_mains_info(
    path: &std::path::Path,
    device_name: &str,
    entries: &mut Vec<ParsedDatum>,
) -> Result<(), String> {
    let online_path = path.join("online");
    if let Ok(online_content) = fs::read_to_string(&online_path) {
        let online = online_content.trim();
        let status = if online == "1" {
            "Connected"
        } else {
            "Disconnected"
        };

        entries.push(ParsedDatum {
            key: format!("AC Power {}", device_name),
            label: format!("AC Power {}", device_name),
            unit: String::new(),
            retention: Retention::Rolling(MAX_SAMPLES),
            value: SeriesValue::Text(status.to_string()),
        });
    }
    Ok(())
}

fn read_usb_power_info(
    path: &std::path::Path,
    device_name: &str,
    entries: &mut Vec<ParsedDatum>,
) -> Result<(), String> {
    let online_path = path.join("online");
    if let Ok(online_content) = fs::read_to_string(&online_path) {
        let online = online_content.trim();
        let status = if online == "1" {
            "Connected"
        } else {
            "Disconnected"
        };

        entries.push(ParsedDatum {
            key: format!("USB Power {}", device_name),
            label: format!("USB Power {}", device_name),
            unit: String::new(),
            retention: Retention::Rolling(MAX_SAMPLES),
            value: SeriesValue::Text(status.to_string()),
        });
    }
    Ok(())
}
