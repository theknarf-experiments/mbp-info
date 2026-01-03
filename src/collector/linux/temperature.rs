use crate::collector::{Collector, MAX_SAMPLES};
use crate::data::{ParsedDatum, Retention, SampleData, SeriesValue};
use std::fs;

pub struct TemperatureCollector;

impl TemperatureCollector {
    pub fn new() -> Self {
        Self
    }
}

impl Collector for TemperatureCollector {
    fn id(&self) -> &'static str {
        "temperature"
    }

    fn name(&self) -> &'static str {
        "Temperature Sensors"
    }

    fn sample(&mut self) -> Result<SampleData, String> {
        let mut entries = Vec::new();

        // Read thermal zones
        if let Ok(thermal_zones) = read_thermal_zones() {
            for (name, temp_celsius) in thermal_zones {
                entries.push(ParsedDatum {
                    key: format!("{}|°C", name),
                    label: name,
                    unit: "°C".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(temp_celsius),
                });
            }
        }

        // Read hwmon sensors
        if let Ok(hwmon_sensors) = read_hwmon_sensors() {
            for (name, temp_celsius) in hwmon_sensors {
                entries.push(ParsedDatum {
                    key: format!("{}|°C", name),
                    label: name,
                    unit: "°C".to_string(),
                    retention: Retention::Rolling(MAX_SAMPLES),
                    value: SeriesValue::Number(temp_celsius),
                });
            }
        }

        Ok(SampleData { entries })
    }
}

fn read_thermal_zones() -> Result<Vec<(String, f64)>, String> {
    let mut sensors = Vec::new();

    // Read thermal zone temperatures
    let thermal_dir = "/sys/class/thermal";
    if let Ok(entries) = fs::read_dir(thermal_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(zone_name) = path.file_name().and_then(|n| n.to_str()) {
                if zone_name.starts_with("thermal_zone") {
                    // Get temperature
                    let temp_path = path.join("temp");
                    if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                        if let Ok(temp_millidegrees) = temp_str.trim().parse::<u64>() {
                            let temp_celsius = temp_millidegrees as f64 / 1000.0;

                            // Get sensor type/name
                            let type_path = path.join("type");
                            let sensor_name = if let Ok(type_str) = fs::read_to_string(&type_path) {
                                format!("Thermal {}", type_str.trim())
                            } else {
                                format!("Thermal {}", zone_name)
                            };

                            sensors.push((sensor_name, temp_celsius));
                        }
                    }
                }
            }
        }
    }

    Ok(sensors)
}

fn read_hwmon_sensors() -> Result<Vec<(String, f64)>, String> {
    let mut sensors = Vec::new();

    // Read hwmon sensor temperatures
    let hwmon_dir = "/sys/class/hwmon";
    if let Ok(hwmon_entries) = fs::read_dir(hwmon_dir) {
        for hwmon_entry in hwmon_entries.flatten() {
            let hwmon_path = hwmon_entry.path();

            // Get device name
            let name_path = hwmon_path.join("name");
            let device_name = if let Ok(name_str) = fs::read_to_string(&name_path) {
                name_str.trim().to_string()
            } else {
                continue;
            };

            // Read all temp*_input files
            if let Ok(temp_entries) = fs::read_dir(&hwmon_path) {
                for temp_entry in temp_entries.flatten() {
                    let temp_path = temp_entry.path();
                    if let Some(filename) = temp_path.file_name().and_then(|n| n.to_str()) {
                        if filename.starts_with("temp") && filename.ends_with("_input") {
                            // Extract sensor number (e.g., "temp1_input" -> "1")
                            let parts: Vec<&str> = filename.split('_').collect();
                            let sensor_num = parts.get(0).and_then(|s| s.strip_prefix("temp"));

                            if let Some(temp_str) = fs::read_to_string(&temp_path).ok() {
                                if let Ok(temp_millidegrees) = temp_str.trim().parse::<u64>() {
                                    let temp_celsius = temp_millidegrees as f64 / 1000.0;

                                    // Try to get a label for this sensor
                                    let label_path = hwmon_path.join(format!(
                                        "{}label",
                                        filename.replace("_input", "_label")
                                    ));
                                    let sensor_label =
                                        if let Ok(label_str) = fs::read_to_string(&label_path) {
                                            format!("{} {}", device_name, label_str.trim())
                                        } else if let Some(num) = sensor_num {
                                            format!("{} temp{}", device_name, num)
                                        } else {
                                            format!("{}", device_name)
                                        };

                                    sensors.push((sensor_label, temp_celsius));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(sensors)
}
