use std::collections::{HashMap, VecDeque};
use std::time::{Duration, SystemTime};

use crate::collector::Collector;
use crate::query::{
    Query, QueryProvider, QueryResult, QueryRow, QuerySource, SeriesData, SeriesKind,
    SeriesSelector,
};
use crate::widget::Widget;

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
pub struct Point {
    pub at: SystemTime,
    pub value: SeriesValue,
}

#[derive(Clone, Debug)]
pub struct DataSeries {
    /// Store-wide id: `<collector>/<key>`.
    pub id: String,
    pub collector: &'static str,
    pub label: String,
    pub unit: String,
    pub retention: Retention,
    pub points: VecDeque<Point>,
    pub sample_count: u64,
}

impl DataSeries {
    fn matches(&self, selector: &SeriesSelector) -> bool {
        let kind_matches = match selector.kind {
            SeriesKind::Any => true,
            SeriesKind::Metric => self.retention.is_rolling(),
            SeriesKind::Info => !self.retention.is_rolling(),
        };
        kind_matches
            && selector
                .collector
                .as_deref()
                .is_none_or(|collector| collector == self.collector)
    }
}

pub fn series_id(collector: &str, key: &str) -> String {
    format!("{collector}/{key}")
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
    pub widgets: Vec<Widget>,
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

    pub fn apply_sample(&mut self, collector: &'static str, at: SystemTime, data: SampleData) {
        for entry in data.entries {
            let id = series_id(collector, &entry.key);
            let is_new = !self.series.contains_key(&id);
            let series = self.series.entry(id.clone()).or_insert_with(|| DataSeries {
                id: id.clone(),
                collector,
                label: entry.label.clone(),
                unit: entry.unit.clone(),
                retention: entry.retention.clone(),
                points: VecDeque::new(),
                sample_count: 0,
            });

            series.label = entry.label;
            series.unit = entry.unit;
            series.retention = entry.retention;

            let point = Point {
                at,
                value: entry.value,
            };
            match series.retention {
                Retention::Rolling(max) => {
                    series.points.push_back(point);
                    while series.points.len() > max {
                        series.points.pop_front();
                    }
                }
                Retention::Latest => {
                    series.points.clear();
                    series.points.push_back(point);
                }
            }

            series.sample_count = series.sample_count.saturating_add(1);

            if is_new {
                self.order.push(id);
            }
        }
    }

    fn select(&self, selector: &SeriesSelector) -> Vec<&DataSeries> {
        let candidates = selector.ids.as_ref().unwrap_or(&self.order);
        candidates
            .iter()
            .filter_map(|id| self.series.get(id))
            .filter(|series| series.matches(selector))
            .collect()
    }

    pub fn metric_count(&self) -> usize {
        self.select(&SeriesSelector::kind(SeriesKind::Metric)).len()
    }

    pub fn info_count(&self) -> usize {
        self.select(&SeriesSelector::kind(SeriesKind::Info)).len()
    }

    pub fn query_at(&self, query: &Query, now: SystemTime) -> QueryResult {
        let QuerySource::Series(selector) = &query.source else {
            return QueryResult::empty("Unsupported");
        };

        let rows = self
            .select(selector)
            .into_iter()
            .map(|series| {
                let points = query.range.select(series.points.iter().cloned(), now);
                let latest = points
                    .last()
                    .map(|point| point.value.clone())
                    .unwrap_or_else(|| SeriesValue::Text("-".to_string()));
                QueryRow {
                    cells: vec![SeriesValue::Text(series.label.clone()), latest],
                    series: Some(SeriesData {
                        id: series.id.clone(),
                        label: series.label.clone(),
                        unit: series.unit.clone(),
                        sample_count: series.sample_count,
                        points,
                    }),
                }
            })
            .collect();

        let (title, name_column) = match selector.kind {
            SeriesKind::Metric => ("Metrics", "Metric"),
            SeriesKind::Info => ("Static Info", "Info"),
            SeriesKind::Any => ("Series", "Series"),
        };

        QueryResult {
            title: title.to_string(),
            columns: vec![name_column.to_string(), "Value".to_string()],
            rows,
            value_column: Some(1),
        }
    }
}

impl QueryProvider for DataStore {
    fn query(&self, query: &Query) -> QueryResult {
        self.query_at(query, SystemTime::now())
    }
}

impl DashboardStore {
    pub fn new() -> Self {
        Self {
            dashboards: vec![Dashboard {
                kind: DashboardType::FixedType,
                widgets: Vec::new(),
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

    pub fn active(&self) -> Option<&Dashboard> {
        self.dashboards.get(self.active)
    }

    pub fn active_type(&self) -> DashboardType {
        self.active()
            .map(|dashboard| dashboard.kind.clone())
            .unwrap_or(DashboardType::FixedType)
    }

    pub fn add_blank(&mut self) {
        self.dashboards.push(Dashboard {
            kind: DashboardType::BlankType,
            widgets: Vec::new(),
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
}

impl QueryProvider for DashboardStore {
    fn query(&self, _query: &Query) -> QueryResult {
        let rows = self
            .dashboards
            .iter()
            .enumerate()
            .map(|(index, dashboard)| QueryRow {
                cells: vec![
                    SeriesValue::Text((index + 1).to_string()),
                    SeriesValue::Text(dashboard.kind.label().to_string()),
                    SeriesValue::Text(if index == self.active {
                        "active".to_string()
                    } else {
                        String::new()
                    }),
                ],
                series: None,
            })
            .collect();

        QueryResult {
            title: "Dashboards".to_string(),
            columns: vec!["#".to_string(), "Type".to_string(), "Active".to_string()],
            rows,
            value_column: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::query::TimeRange;

    fn datum(key: &str, value: f64) -> ParsedDatum {
        ParsedDatum {
            key: key.to_string(),
            label: key.to_string(),
            unit: "W".to_string(),
            retention: Retention::Rolling(10),
            value: SeriesValue::Number(value),
        }
    }

    fn sample(entries: Vec<ParsedDatum>) -> SampleData {
        SampleData { entries }
    }

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn values(result: &QueryResult) -> Vec<Vec<f64>> {
        result
            .rows
            .iter()
            .map(|row| {
                row.series
                    .as_ref()
                    .unwrap()
                    .points
                    .iter()
                    .map(|point| match point.value {
                        SeriesValue::Number(value) => value,
                        SeriesValue::Text(_) => f64::NAN,
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn same_key_from_different_collectors_stays_separate() {
        let mut store = DataStore::new();
        store.apply_sample("a", at(1), sample(vec![datum("power", 1.0)]));
        store.apply_sample("b", at(1), sample(vec![datum("power", 2.0)]));

        let mut selector = SeriesSelector::kind(SeriesKind::Any);
        selector.collector = Some("b".to_string());
        let result = store.query_at(&Query::series(selector, TimeRange::All), at(1));
        assert_eq!(values(&result), vec![vec![2.0]]);
        assert_eq!(result.rows[0].series.as_ref().unwrap().id, "b/power");
    }

    #[test]
    fn range_queries_filter_by_timestamp() {
        let mut store = DataStore::new();
        for secs in 0..10 {
            store.apply_sample("a", at(secs), sample(vec![datum("power", secs as f64)]));
        }
        let selector = || SeriesSelector::ids(vec!["a/power".to_string()]);
        let now = at(9);

        let latest = store.query_at(&Query::series(selector(), TimeRange::Latest), now);
        assert_eq!(values(&latest), vec![vec![9.0]]);

        let last = TimeRange::Last(Duration::from_secs(2));
        let last = store.query_at(&Query::series(selector(), last), now);
        assert_eq!(values(&last), vec![vec![7.0, 8.0, 9.0]]);

        let between = TimeRange::Between {
            from: at(3),
            to: at(5),
        };
        let between = store.query_at(&Query::series(selector(), between), now);
        assert_eq!(values(&between), vec![vec![3.0, 4.0, 5.0]]);
    }

    #[test]
    fn id_selector_keeps_requested_order() {
        let mut store = DataStore::new();
        store.apply_sample(
            "a",
            at(1),
            sample(vec![datum("first", 1.0), datum("second", 2.0)]),
        );

        let selector = SeriesSelector::ids(vec!["a/second".to_string(), "a/first".to_string()]);
        let result = store.query_at(&Query::series(selector, TimeRange::Latest), at(1));
        assert_eq!(values(&result), vec![vec![2.0], vec![1.0]]);
    }
}
