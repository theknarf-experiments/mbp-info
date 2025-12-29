use crate::data::SeriesValue;

#[derive(Copy, Clone, Debug)]
pub enum QuerySource {
    Metrics,
    StaticInfo,
    Collectors,
    Dashboards,
}

#[derive(Copy, Clone, Debug)]
pub enum SeriesMode {
    Latest,
    Full,
}

#[derive(Copy, Clone, Debug)]
pub struct Query {
    pub source: QuerySource,
    pub mode: SeriesMode,
    pub selected_index: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct QueryRow {
    pub cells: Vec<Vec<SeriesValue>>,
    pub series_label: Option<String>,
    pub series_unit: Option<String>,
    pub sample_count: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct QueryResult {
    pub title: String,
    pub columns: Vec<String>,
    pub rows: Vec<QueryRow>,
    pub series_column: Option<usize>,
}

pub trait QueryProvider {
    fn query(&self, query: Query) -> QueryResult;
}
