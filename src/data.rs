use std::collections::{HashMap, VecDeque};

pub const MAX_SAMPLES: usize = 120;

#[derive(Clone)]
pub struct MetricSeries {
    pub label: String,
    pub unit: String,
    pub values: VecDeque<f64>,
}

#[derive(Clone)]
pub struct ParsedMetric {
    pub key: String,
    pub label: String,
    pub unit: String,
    pub value: f64,
}

#[derive(Clone)]
pub struct ParsedInfo {
    pub label: String,
    pub value: String,
}

#[derive(Clone)]
pub struct SampleData {
    pub metrics: Vec<ParsedMetric>,
    pub info: Vec<ParsedInfo>,
}

pub struct DataStore {
    metrics: HashMap<String, MetricSeries>,
    order: Vec<String>,
    metric_samples: HashMap<String, u64>,
    info: HashMap<String, String>,
    info_order: Vec<String>,
}

impl DataStore {
    pub fn new() -> Self {
        Self {
            metrics: HashMap::new(),
            order: Vec::new(),
            metric_samples: HashMap::new(),
            info: HashMap::new(),
            info_order: Vec::new(),
        }
    }

    pub fn apply_sample(&mut self, data: SampleData) {
        for metric in data.metrics {
            let entry = self
                .metrics
                .entry(metric.key.clone())
                .or_insert_with(|| MetricSeries {
                    label: metric.label.clone(),
                    unit: metric.unit.clone(),
                    values: VecDeque::new(),
                });
            entry.values.push_back(metric.value);
            while entry.values.len() > MAX_SAMPLES {
                entry.values.pop_front();
            }

            let count = self.metric_samples.entry(metric.key.clone()).or_insert(0);
            *count += 1;

            if !self.order.contains(&metric.key) {
                self.order.push(metric.key);
            }
        }

        for entry in data.info {
            let is_new = !self.info.contains_key(&entry.label);
            self.info.insert(entry.label.clone(), entry.value);
            if is_new {
                self.info_order.push(entry.label);
            }
        }
    }

    pub fn metrics(&self) -> &HashMap<String, MetricSeries> {
        &self.metrics
    }

    pub fn metric_order(&self) -> &[String] {
        &self.order
    }

    pub fn metric_count(&self) -> usize {
        self.order.len()
    }

    pub fn metric_key(&self, index: usize) -> Option<&str> {
        self.order.get(index).map(String::as_str)
    }

    pub fn info(&self) -> &HashMap<String, String> {
        &self.info
    }

    pub fn info_order(&self) -> &[String] {
        &self.info_order
    }

    pub fn metric_sample_count(&self, key: &str) -> u64 {
        self.metric_samples.get(key).copied().unwrap_or(0)
    }
}
