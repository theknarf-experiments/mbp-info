use std::time::{Duration, SystemTime};

use crate::data::{DashboardStore, DataStore, Point, SeriesValue};

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SeriesKind {
    Any,
    /// Series that keep a rolling history (numbers over time).
    Metric,
    /// Series that only keep their latest value (static info).
    Info,
}

/// Picks which series a query returns. Every set field must match.
#[derive(Clone, Debug)]
pub struct SeriesSelector {
    pub kind: SeriesKind,
    pub collector: Option<String>,
    /// Exact series ids, returned in the order given. `None` matches every
    /// series.
    pub ids: Option<Vec<String>>,
}

impl SeriesSelector {
    pub fn kind(kind: SeriesKind) -> Self {
        Self {
            kind,
            collector: None,
            ids: None,
        }
    }

    pub fn ids(ids: Vec<String>) -> Self {
        Self {
            kind: SeriesKind::Any,
            collector: None,
            ids: Some(ids),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub enum TimeRange {
    /// Only the most recent point of each series.
    Latest,
    /// Every retained point.
    All,
    /// Points no older than the duration.
    #[allow(dead_code)] // Not used by the built-in dashboard yet.
    Last(Duration),
    /// Points with `from <= at <= to`.
    #[allow(dead_code)]
    Between { from: SystemTime, to: SystemTime },
}

impl TimeRange {
    pub fn select(
        &self,
        points: impl DoubleEndedIterator<Item = Point>,
        now: SystemTime,
    ) -> Vec<Point> {
        match *self {
            TimeRange::Latest => points.last().into_iter().collect(),
            TimeRange::All => points.collect(),
            TimeRange::Last(duration) => {
                let from = now.checked_sub(duration).unwrap_or(SystemTime::UNIX_EPOCH);
                points.filter(|point| point.at >= from).collect()
            }
            TimeRange::Between { from, to } => points
                .filter(|point| point.at >= from && point.at <= to)
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum QuerySource {
    Series(SeriesSelector),
    Collectors,
    Dashboards,
}

#[derive(Clone, Debug)]
pub struct Query {
    pub source: QuerySource,
    pub range: TimeRange,
}

impl Query {
    pub fn series(selector: SeriesSelector, range: TimeRange) -> Self {
        Self {
            source: QuerySource::Series(selector),
            range,
        }
    }

    pub fn collectors() -> Self {
        Self {
            source: QuerySource::Collectors,
            range: TimeRange::Latest,
        }
    }

    pub fn dashboards() -> Self {
        Self {
            source: QuerySource::Dashboards,
            range: TimeRange::Latest,
        }
    }
}

/// The series behind a result row, with the points that fell in range.
#[derive(Clone, Debug)]
pub struct SeriesData {
    pub id: String,
    pub label: String,
    pub unit: String,
    pub sample_count: u64,
    pub points: Vec<Point>,
}

#[derive(Clone, Debug)]
pub struct QueryRow {
    pub cells: Vec<SeriesValue>,
    pub series: Option<SeriesData>,
}

#[derive(Clone, Debug)]
pub struct QueryResult {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<QueryRow>,
    /// Column holding the series value, formatted with the series unit.
    pub value_column: Option<usize>,
}

impl QueryResult {
    pub fn empty(title: &str) -> Self {
        Self {
            title: title.to_string(),
            columns: Vec::new(),
            rows: Vec::new(),
            value_column: None,
        }
    }
}

pub trait QueryProvider {
    fn query(&self, query: &Query) -> QueryResult;
}

/// Routes each query to the store that owns its source.
pub struct QueryContext<'a> {
    pub data: &'a DataStore,
    pub collectors: &'a dyn QueryProvider,
    pub dashboards: &'a DashboardStore,
}

impl QueryProvider for QueryContext<'_> {
    fn query(&self, query: &Query) -> QueryResult {
        match query.source {
            QuerySource::Series(_) => self.data.query(query),
            QuerySource::Collectors => self.collectors.query(query),
            QuerySource::Dashboards => self.dashboards.query(query),
        }
    }
}
