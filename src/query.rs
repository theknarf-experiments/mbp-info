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
    /// Case-insensitive glob over series ids (`*` and `?` wildcards).
    pub pattern: Option<String>,
}

impl SeriesSelector {
    pub fn kind(kind: SeriesKind) -> Self {
        Self {
            kind,
            collector: None,
            ids: None,
            pattern: None,
        }
    }

    pub fn pattern(pattern: impl Into<String>) -> Self {
        Self {
            kind: SeriesKind::Any,
            collector: None,
            ids: None,
            pattern: Some(pattern.into()),
        }
    }

    pub fn ids(ids: Vec<String>) -> Self {
        Self {
            kind: SeriesKind::Any,
            collector: None,
            ids: Some(ids),
            pattern: None,
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
    Last(Duration),
    /// Points with `from <= at <= to`.
    #[allow(dead_code)] // No command creates absolute ranges yet.
    Between { from: SystemTime, to: SystemTime },
}

impl TimeRange {
    /// Parses `latest`, `all`, or a duration like `30s`, `5m`, `2h`.
    pub fn parse(input: &str) -> Option<Self> {
        match input.to_ascii_lowercase().as_str() {
            "latest" => return Some(TimeRange::Latest),
            "all" => return Some(TimeRange::All),
            _ => {}
        }
        let unit = input.chars().last()?;
        let multiplier = match unit {
            's' => 1,
            'm' => 60,
            'h' => 3600,
            _ => return None,
        };
        let amount: u64 = input[..input.len() - 1].parse().ok()?;
        if amount == 0 {
            return None;
        }
        Some(TimeRange::Last(Duration::from_secs(amount * multiplier)))
    }

    pub fn describe(&self) -> String {
        match self {
            TimeRange::Latest => "latest".to_string(),
            TimeRange::All => "all".to_string(),
            TimeRange::Last(duration) => {
                let secs = duration.as_secs();
                if secs % 3600 == 0 {
                    format!("last {}h", secs / 3600)
                } else if secs % 60 == 0 {
                    format!("last {}m", secs / 60)
                } else {
                    format!("last {secs}s")
                }
            }
            TimeRange::Between { .. } => "between".to_string(),
        }
    }

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

/// Case-insensitive glob match supporting `*` (any run) and `?` (one char).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.to_lowercase().chars().collect();
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut backtrack: Option<(usize, usize)> = None;

    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            backtrack = Some((p, t));
            p += 1;
        } else if let Some((star, matched)) = backtrack {
            p = star + 1;
            t = matched + 1;
            backtrack = Some((star, t));
        } else {
            return false;
        }
    }

    pattern[p..].iter().all(|ch| *ch == '*')
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_matches_wildcards_case_insensitively() {
        assert!(glob_match("*", "anything/at all"));
        assert!(glob_match(
            "powermetrics/*",
            "powermetrics/CPU die temperature|C"
        ));
        assert!(glob_match(
            "cpu_usage/cpu core ?? usage*",
            "cpu_usage/CPU Core 12 usage|%"
        ));
        assert!(!glob_match(
            "cpu_usage/cpu core ?? usage*",
            "cpu_usage/CPU Core 1 usage|%"
        ));
        assert!(glob_match("*die*", "powermetrics/GPU die temperature|C"));
        assert!(!glob_match("memory_usage/*", "swap_usage/Swap Used|MB"));
    }

    #[test]
    fn parses_ranges() {
        assert!(matches!(
            TimeRange::parse("latest"),
            Some(TimeRange::Latest)
        ));
        assert!(matches!(TimeRange::parse("ALL"), Some(TimeRange::All)));
        assert!(matches!(
            TimeRange::parse("5m"),
            Some(TimeRange::Last(duration)) if duration == Duration::from_secs(300)
        ));
        assert!(TimeRange::parse("0s").is_none());
        assert!(TimeRange::parse("5x").is_none());
        assert!(TimeRange::parse("powermetrics/*").is_none());
    }
}
