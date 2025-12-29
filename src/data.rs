use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use crate::collector::Collector;

pub const MAX_SAMPLES: usize = 120;

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

pub enum QueryKind {
    Table,
    Series,
}

pub enum QuerySource {
    Metrics,
    StaticInfo,
    Collectors,
}

pub struct Query {
    pub kind: QueryKind,
    pub source: QuerySource,
    pub selected_index: Option<usize>,
    pub show_samples: bool,
}

impl Query {
    pub fn metrics_table(selected_index: usize, show_samples: bool) -> Self {
        Self {
            kind: QueryKind::Table,
            source: QuerySource::Metrics,
            selected_index: Some(selected_index),
            show_samples,
        }
    }

    pub fn static_info_table(selected_index: usize) -> Self {
        Self {
            kind: QueryKind::Table,
            source: QuerySource::StaticInfo,
            selected_index: Some(selected_index),
            show_samples: false,
        }
    }

    pub fn collectors_table(selected_index: usize) -> Self {
        Self {
            kind: QueryKind::Table,
            source: QuerySource::Collectors,
            selected_index: Some(selected_index),
            show_samples: false,
        }
    }

    pub fn metrics_series(selected_index: usize) -> Self {
        Self {
            kind: QueryKind::Series,
            source: QuerySource::Metrics,
            selected_index: Some(selected_index),
            show_samples: false,
        }
    }
}

pub struct TableResult {
    pub title: String,
    pub rows: Vec<Vec<String>>,
    pub right_align_last: bool,
}

pub struct SeriesView {
    pub label: String,
    pub points: Vec<(f64, f64)>,
}

pub struct SeriesResult {
    pub title: String,
    pub series: Vec<SeriesView>,
}

pub enum QueryResult {
    Table(TableResult),
    Series(SeriesResult),
}

pub trait QueryProvider {
    fn query(&self, query: Query) -> QueryResult;
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

    pub fn query(&self, query: Query) -> QueryResult {
        match (query.kind, query.source) {
            (QueryKind::Table, QuerySource::Metrics) => {
                QueryResult::Table(self.metrics_table(query.selected_index, query.show_samples))
            }
            (QueryKind::Table, QuerySource::StaticInfo) => {
                QueryResult::Table(self.static_info_table(query.selected_index))
            }
            (QueryKind::Table, QuerySource::Collectors) => QueryResult::Table(TableResult {
                title: "Collectors".to_string(),
                rows: Vec::new(),
                right_align_last: true,
            }),
            (QueryKind::Series, QuerySource::Metrics) => {
                QueryResult::Series(self.metrics_series(query.selected_index))
            }
            _ => QueryResult::Series(SeriesResult {
                title: "Series".to_string(),
                series: Vec::new(),
            }),
        }
    }

    fn metrics_table(&self, selected_index: Option<usize>, show_samples: bool) -> TableResult {
        let selected = selected_index.unwrap_or(usize::MAX);
        let entries = self.metric_series();
        let rows = entries
            .iter()
            .enumerate()
            .map(|(index, series)| {
                let samples = if show_samples && index == selected {
                    sample_label(series.sample_count)
                } else {
                    String::new()
                };
                vec![series.label.clone(), series.display_value(), samples]
            })
            .collect();

        TableResult {
            title: "Metrics".to_string(),
            rows,
            right_align_last: true,
        }
    }

    fn static_info_table(&self, _selected_index: Option<usize>) -> TableResult {
        let rows = self
            .info_series()
            .iter()
            .map(|series| vec![series.label.clone(), series.display_value()])
            .collect();

        TableResult {
            title: "Static Info".to_string(),
            rows,
            right_align_last: false,
        }
    }

    fn metrics_series(&self, selected_index: Option<usize>) -> SeriesResult {
        let Some(selected_index) = selected_index else {
            return SeriesResult {
                title: "Chart".to_string(),
                series: Vec::new(),
            };
        };
        let Some(selected) = self.metric_series_at(selected_index) else {
            return SeriesResult {
                title: "Chart".to_string(),
                series: Vec::new(),
            };
        };

        if let Some((group_name, labels)) = group_rule_for_label(&selected.label) {
            let mut grouped = Vec::new();
            for label in labels {
                if let Some(found) = self.metric_series().into_iter().find(|metric| {
                    label_matches(&metric.label, label) && metric.unit == selected.unit
                }) {
                    grouped.push(found);
                }
            }
            if !grouped.is_empty() {
                let title = if selected.unit.is_empty() {
                    group_name.to_string()
                } else {
                    format!("{group_name} ({})", selected.unit)
                };
                return SeriesResult {
                    title,
                    series: grouped
                        .iter()
                        .map(|series| build_series_view(series))
                        .collect(),
                };
            }
        }

        let title = if selected.unit.is_empty() {
            selected.label.clone()
        } else {
            format!("{} ({})", selected.label, selected.unit)
        };

        SeriesResult {
            title,
            series: vec![build_series_view(selected)],
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

impl QueryProvider for DataStore {
    fn query(&self, query: Query) -> QueryResult {
        DataStore::query(self, query)
    }
}

fn format_number(value: f64) -> String {
    let fixed = format!("{value:.2}");
    fixed
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn sample_label(count: u64) -> String {
    if count == 1 {
        "1 sample".to_string()
    } else {
        format!("{count} samples")
    }
}

fn build_series_view(series: &DataSeries) -> SeriesView {
    let points = series
        .values
        .iter()
        .enumerate()
        .filter_map(|(index, value)| match value {
            SeriesValue::Number(value) => Some((index as f64, *value)),
            SeriesValue::Text(_) => None,
        })
        .collect();
    SeriesView {
        label: series.label.clone(),
        points,
    }
}

fn label_matches(label: &str, other: &str) -> bool {
    label.eq_ignore_ascii_case(other)
}

fn group_rule_for_label(label: &str) -> Option<(&'static str, &'static [&'static str])> {
    GROUP_RULES
        .iter()
        .find(|(_, labels)| labels.iter().any(|entry| label_matches(label, entry)))
        .map(|(name, labels)| (*name, *labels))
}

const GROUP_RULES: &[(&str, &[&str])] = &[(
    "Die temperature",
    &["CPU die temperature", "GPU die temperature"],
)];
