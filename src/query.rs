use crate::data::{DataSeries, DataStore};

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

    pub fn metrics_series_all() -> Self {
        Self {
            kind: QueryKind::Series,
            source: QuerySource::Metrics,
            selected_index: None,
            show_samples: false,
        }
    }
}

pub struct TableResult {
    pub title: String,
    pub rows: Vec<Vec<String>>,
    pub right_align_last: bool,
}

#[derive(Clone)]
pub struct SeriesView {
    pub label: String,
    pub unit: String,
    pub points: Vec<(f64, f64)>,
}

#[derive(Clone)]
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

impl DataStore {
    pub fn query(&self, query: Query) -> QueryResult {
        match (query.kind, query.source) {
            (QueryKind::Table, QuerySource::Metrics) => {
                QueryResult::Table(self.metrics_table(query.selected_index, query.show_samples))
            }
            (QueryKind::Table, QuerySource::StaticInfo) => {
                QueryResult::Table(self.static_info_table())
            }
            (QueryKind::Series, QuerySource::Metrics) => {
                if query.selected_index.is_some() {
                    QueryResult::Series(self.metrics_series(query.selected_index))
                } else {
                    QueryResult::Series(self.metrics_series_all())
                }
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

    fn static_info_table(&self) -> TableResult {
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
        if let Some(index) = selected_index {
            if let Some(selected) = self.metric_series_at(index) {
                let title = if selected.unit.is_empty() {
                    selected.label.clone()
                } else {
                    format!("{} ({})", selected.label, selected.unit)
                };
                return SeriesResult {
                    title,
                    series: vec![build_series_view(selected)],
                };
            }
        }

        SeriesResult {
            title: "Chart".to_string(),
            series: Vec::new(),
        }
    }

    fn metrics_series_all(&self) -> SeriesResult {
        SeriesResult {
            title: "Metrics".to_string(),
            series: self
                .metric_series()
                .iter()
                .map(|series| build_series_view(series))
                .collect(),
        }
    }
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
            crate::data::SeriesValue::Number(value) => Some((index as f64, *value)),
            crate::data::SeriesValue::Text(_) => None,
        })
        .collect();
    SeriesView {
        label: series.label.clone(),
        unit: series.unit.clone(),
        points,
    }
}
