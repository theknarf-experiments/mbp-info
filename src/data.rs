use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use crate::collector::Collector;
use crate::query::{Query, QueryResult, QueryRow, QuerySource, SeriesMode};

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

#[derive(Clone, Debug)]
pub enum DashboardType {
    FixedType,
    BlankType,
}

impl DashboardType {
    pub fn label(&self) -> &'static str {
        match self {
            DashboardType::FixedType => "Fixed",
            DashboardType::BlankType => "Blank",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Dashboard {
    pub kind: DashboardType,
}

pub struct DashboardStore {
    dashboards: Vec<Dashboard>,
    active: usize,
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

    pub fn query(&self, query: Query) -> QueryResult {
        match query.source {
            QuerySource::Metrics => self.metrics_query(query.selected_index, query.mode),
            QuerySource::StaticInfo => self.static_info_query(query.mode),
            QuerySource::Collectors => QueryResult {
                title: "Collectors".to_string(),
                columns: Vec::new(),
                rows: Vec::new(),
                series_column: None,
            },
            QuerySource::Dashboards => QueryResult {
                title: "Dashboards".to_string(),
                columns: Vec::new(),
                rows: Vec::new(),
                series_column: None,
            },
        }
    }

    fn metrics_query(&self, selected_index: Option<usize>, mode: SeriesMode) -> QueryResult {
        let entries = self.metric_series();
        let selected_entries: Vec<&DataSeries> = match mode {
            SeriesMode::Latest => entries,
            SeriesMode::Full => {
                if let Some(index) = selected_index {
                    entries.get(index).copied().into_iter().collect()
                } else {
                    entries
                }
            }
        };

        let rows = selected_entries
            .iter()
            .map(|series| QueryRow {
                cells: vec![
                    vec![SeriesValue::Text(series.label.clone())],
                    series_values(series, mode),
                    vec![SeriesValue::Number(series.sample_count as f64)],
                ],
                series_label: Some(series.label.clone()),
                series_unit: Some(series.unit.clone()),
                sample_count: Some(series.sample_count),
            })
            .collect();

        let title = match mode {
            SeriesMode::Latest => "Metrics".to_string(),
            SeriesMode::Full => {
                if let Some(index) = selected_index {
                    if let Some(selected) = self.metric_series_at(index) {
                        if selected.unit.is_empty() {
                            selected.label.clone()
                        } else {
                            format!("{} ({})", selected.label, selected.unit)
                        }
                    } else {
                        "Chart".to_string()
                    }
                } else {
                    "Metrics".to_string()
                }
            }
        };

        QueryResult {
            title,
            columns: vec![
                "Metric".to_string(),
                "Value".to_string(),
                "Samples".to_string(),
            ],
            rows,
            series_column: Some(1),
        }
    }

    fn static_info_query(&self, mode: SeriesMode) -> QueryResult {
        let rows = self
            .info_series()
            .iter()
            .map(|series| QueryRow {
                cells: vec![
                    vec![SeriesValue::Text(series.label.clone())],
                    series_values(series, mode),
                ],
                series_label: Some(series.label.clone()),
                series_unit: Some(series.unit.clone()),
                sample_count: Some(series.sample_count),
            })
            .collect();

        QueryResult {
            title: "Static Info".to_string(),
            columns: vec!["Info".to_string(), "Value".to_string()],
            rows,
            series_column: Some(1),
        }
    }
}

fn series_values(series: &DataSeries, mode: SeriesMode) -> Vec<SeriesValue> {
    match mode {
        SeriesMode::Latest => series.values.back().cloned().into_iter().collect(),
        SeriesMode::Full => series.values.iter().cloned().collect(),
    }
}

impl DashboardStore {
    pub fn new() -> Self {
        Self {
            dashboards: vec![Dashboard {
                kind: DashboardType::FixedType,
            }],
            active: 0,
        }
    }

    pub fn count(&self) -> usize {
        self.dashboards.len()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active_type(&self) -> DashboardType {
        self.dashboards
            .get(self.active)
            .map(|dashboard| dashboard.kind.clone())
            .unwrap_or(DashboardType::FixedType)
    }

    pub fn add_blank(&mut self) {
        self.dashboards.push(Dashboard {
            kind: DashboardType::BlankType,
        });
        self.active = self.dashboards.len().saturating_sub(1);
    }

    pub fn remove(&mut self, index: usize) -> Result<(), String> {
        if self.dashboards.len() <= 1 {
            return Err("Cannot remove the last dashboard.".to_string());
        }
        if index >= self.dashboards.len() {
            return Err(format!("Dashboard {} does not exist.", index + 1));
        }

        self.dashboards.remove(index);
        if self.active == index {
            if index >= self.dashboards.len() {
                self.active = self.dashboards.len().saturating_sub(1);
            }
        } else if self.active > index {
            self.active = self.active.saturating_sub(1);
        }

        Ok(())
    }

    pub fn set_active(&mut self, index: usize) -> Result<(), String> {
        if index >= self.dashboards.len() {
            return Err(format!("Dashboard {} does not exist.", index + 1));
        }
        self.active = index;
        Ok(())
    }

    pub fn next(&mut self) {
        if self.dashboards.is_empty() {
            return;
        }
        self.active = (self.active + 1) % self.dashboards.len();
    }

    pub fn prev(&mut self) {
        if self.dashboards.is_empty() {
            return;
        }
        if self.active == 0 {
            self.active = self.dashboards.len() - 1;
        } else {
            self.active -= 1;
        }
    }

    pub fn query(&self, _query: Query) -> QueryResult {
        let rows = self
            .dashboards
            .iter()
            .enumerate()
            .map(|(index, dashboard)| QueryRow {
                cells: vec![
                    vec![SeriesValue::Text((index + 1).to_string())],
                    vec![SeriesValue::Text(dashboard.kind.label().to_string())],
                    vec![SeriesValue::Text(if index == self.active {
                        "active".to_string()
                    } else {
                        String::new()
                    })],
                ],
                series_label: None,
                series_unit: None,
                sample_count: None,
            })
            .collect();

        QueryResult {
            title: "Dashboards".to_string(),
            columns: vec!["#".to_string(), "Type".to_string(), "Active".to_string()],
            rows,
            series_column: None,
        }
    }
}
