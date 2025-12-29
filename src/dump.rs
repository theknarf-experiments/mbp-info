use std::fs::File;
use std::io::{self, Write};

use crate::collector::needs_sudo;
use crate::data::{CollectorRegistration, SeriesValue};
use crate::sudo::prompt_for_sudo;

pub struct DumpOptions {
    pub path: String,
    pub collectors: Option<Vec<String>>,
}

pub fn dump_options_from_args(args: &[String]) -> Result<Option<DumpOptions>, String> {
    let mut path: Option<String> = None;
    let mut collectors: Option<Vec<String>> = None;
    let mut index = 1;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--dump-sample" {
            if index + 1 < args.len() && !args[index + 1].starts_with('-') {
                path = Some(args[index + 1].clone());
                index += 2;
                continue;
            }
            path = Some("mbp-info-sample.txt".to_string());
            index += 1;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--dump-sample=") {
            let value = if value.is_empty() {
                "mbp-info-sample.txt"
            } else {
                value
            };
            path = Some(value.to_string());
            index += 1;
            continue;
        }
        if arg == "--collectors" {
            if index + 1 >= args.len() {
                return Err("Missing value for --collectors".to_string());
            }
            collectors = Some(parse_collectors_arg(&args[index + 1])?);
            index += 2;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--collectors=") {
            collectors = Some(parse_collectors_arg(value)?);
            index += 1;
            continue;
        }
        index += 1;
    }

    if collectors.is_some() && path.is_none() {
        return Err("--collectors requires --dump-sample".to_string());
    }

    Ok(path.map(|path| DumpOptions { path, collectors }))
}

fn parse_collectors_arg(value: &str) -> Result<Vec<String>, String> {
    let ids: Vec<String> = value
        .split(',')
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .collect();
    if ids.is_empty() {
        return Err("Collector list is empty.".to_string());
    }
    Ok(ids)
}

pub fn dump_samples(
    options: &DumpOptions,
    registrations: &[CollectorRegistration],
) -> io::Result<()> {
    let use_sudo = needs_sudo();
    let mut sudo_ready = false;
    let selected = selected_collectors(registrations, options.collectors.as_ref())?;

    if use_sudo
        && selected
            .iter()
            .any(|registration| registration.requires_sudo)
    {
        sudo_ready = prompt_for_sudo("Some collectors need sudo. Authenticate now?")?;
    }

    let mut file = File::create(&options.path)?;
    writeln!(file, "mbp-info sample dump")?;

    for registration in selected {
        if registration.requires_sudo && use_sudo && !sudo_ready {
            writeln!(
                file,
                "[{}] {}: skipped (sudo required)",
                registration.id, registration.name
            )?;
            continue;
        }

        let mut collector = (registration.factory)();
        match collector.sample() {
            Ok(sample) => {
                writeln!(file, "\n[{}] {}", registration.id, registration.name)?;
                for entry in sample.entries {
                    writeln!(
                        file,
                        "- {}: {}",
                        entry.label,
                        format_series_value(&entry.value, &entry.unit)
                    )?;
                }
            }
            Err(error) => {
                writeln!(
                    file,
                    "\n[{}] {}: error: {}",
                    registration.id, registration.name, error
                )?;
            }
        }
    }

    Ok(())
}

fn selected_collectors<'a>(
    registrations: &'a [CollectorRegistration],
    filter: Option<&Vec<String>>,
) -> io::Result<Vec<&'a CollectorRegistration>> {
    if let Some(filter) = filter {
        let mut selected = Vec::new();
        for id in filter {
            let Some(found) = registrations.iter().find(|reg| reg.id == id) else {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("Unknown collector id: {id}"),
                ));
            };
            selected.push(found);
        }
        return Ok(selected);
    }

    Ok(registrations.iter().collect())
}

fn format_series_value(value: &SeriesValue, unit: &str) -> String {
    match value {
        SeriesValue::Number(value) => {
            if unit.is_empty() {
                format_number(*value)
            } else {
                format!("{} {}", format_number(*value), unit)
            }
        }
        SeriesValue::Text(value) => value.clone(),
    }
}

fn format_number(value: f64) -> String {
    let fixed = format!("{value:.2}");
    fixed
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
