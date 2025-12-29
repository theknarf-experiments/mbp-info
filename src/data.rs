use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use crate::collector::Collector;

#[derive(Clone, Debug)]
pub enum Retention {
    Rolling(usize),
    Latest,
}

impl Retention {
    pub fn is_rolling(&self) -> bool {
        matches!(self, Retention::Rolling(_))
    }
}

#[derive(Clone, Debug)]
pub enum SeriesValue {
    Number(f64),
    Text(String),
}

#[derive(Clone, Debug)]
pub struct DataSeries {
    pub label: String,
    pub unit: String,
    pub retention: Retention,
    pub values: VecDeque<SeriesValue>,
    pub sample_count: u64,
}

#[derive(Clone, Debug)]
pub struct ParsedDatum {
    pub key: String,
    pub label: String,
    pub unit: String,
    pub retention: Retention,
    pub value: SeriesValue,
}

#[derive(Clone, Debug)]
pub struct SampleData {
    pub entries: Vec<ParsedDatum>,
}

pub struct DataStore {
    series: HashMap<String, DataSeries>,
    order: Vec<String>,
}

pub struct CollectorRegistration {
    pub id: &'static str,
    pub name: &'static str,
    pub requires_sudo: bool,
    pub interval: Duration,
    pub factory: Box<dyn Fn() -> Box<dyn Collector> + Send + Sync>,
}

impl CollectorRegistration {
    pub fn new(factory: Box<dyn Fn() -> Box<dyn Collector> + Send + Sync>) -> Self {
        let collector = factory();
        Self {
            id: collector.id(),
            name: collector.name(),
            requires_sudo: collector.requires_sudo(),
            interval: collector.interval(),
            factory,
        }
    }
}

impl DataStore {
    pub fn new() -> Self {
        Self {
            series: HashMap::new(),
            order: Vec::new(),
        }
    }

    pub fn apply_sample(&mut self, data: SampleData) {
        for entry in data.entries {
            let is_new = !self.series.contains_key(&entry.key);
            let series = self
                .series
                .entry(entry.key.clone())
                .or_insert_with(|| DataSeries {
                    label: entry.label.clone(),
                    unit: entry.unit.clone(),
                    retention: entry.retention.clone(),
                    values: VecDeque::new(),
                    sample_count: 0,
                });

            series.label = entry.label.clone();
            series.unit = entry.unit.clone();
            series.retention = entry.retention.clone();

            match series.retention {
                Retention::Rolling(max) => {
                    series.values.push_back(entry.value);
                    while series.values.len() > max {
                        series.values.pop_front();
                    }
                }
                Retention::Latest => {
                    series.values.clear();
                    series.values.push_back(entry.value);
                }
            }

            series.sample_count = series.sample_count.saturating_add(1);

            if is_new {
                self.order.push(entry.key);
            }
        }
    }

    pub fn metric_series(&self) -> Vec<&DataSeries> {
        self.order
            .iter()
            .filter_map(|key| {
                self.series
                    .get(key)
                    .filter(|series| series.retention.is_rolling())
            })
            .collect()
    }

    pub fn info_series(&self) -> Vec<&DataSeries> {
        self.order
            .iter()
            .filter_map(|key| {
                self.series
                    .get(key)
                    .filter(|series| !series.retention.is_rolling())
            })
            .collect()
    }

    pub fn metric_count(&self) -> usize {
        self.metric_series().len()
    }

    pub fn info_count(&self) -> usize {
        self.info_series().len()
    }

    pub fn metric_series_at(&self, index: usize) -> Option<&DataSeries> {
        let metrics = self.metric_series();
        metrics.get(index).copied()
    }
}

impl DataSeries {
    pub fn display_value(&self) -> String {
        match self.values.back() {
            Some(SeriesValue::Number(value)) => {
                if self.unit.is_empty() {
                    format_number(*value)
                } else {
                    format!("{} {}", format_number(*value), self.unit)
                }
            }
            Some(SeriesValue::Text(value)) => value.clone(),
            None => "-".to_string(),
        }
    }
}

fn format_number(value: f64) -> String {
    let fixed = format!("{value:.2}");
    fixed
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}
