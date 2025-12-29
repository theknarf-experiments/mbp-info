use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Chart, Dataset, List, ListItem, ListState, Paragraph, Wrap,
    },
    symbols,
};

use crate::collector::SAMPLE_INTERVAL;
use crate::data::{DataStore, MetricSeries};

const CHART_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::LightBlue,
    Color::LightRed,
];

static GROUP_RULES: &[(&str, &[&str])] = &[
    ("Die temperature", &["CPU die temperature", "GPU die temperature"]),
];

pub struct AppState {
    selected: usize,
    error: Option<String>,
    use_sudo: bool,
}

impl AppState {
    pub fn new(use_sudo: bool) -> Self {
        Self {
            selected: 0,
            error: None,
            use_sudo,
        }
    }

    pub fn select_next(&mut self, data: &DataStore) {
        let count = data.metric_count();
        if count == 0 {
            return;
        }
        self.selected = (self.selected + 1).min(count - 1);
    }

    pub fn select_prev(&mut self, data: &DataStore) {
        if data.metric_count() == 0 {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn clamp_selection(&mut self, data: &DataStore) {
        let count = data.metric_count();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }
    }

    pub fn selected_key<'a>(&self, data: &'a DataStore) -> Option<&'a str> {
        data.metric_key(self.selected)
    }

    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn use_sudo(&self) -> bool {
        self.use_sudo
    }
}

pub fn draw_ui(frame: &mut Frame, data: &DataStore, app: &AppState) {
    let size = frame.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(size);

    let controls = if app.use_sudo() {
        "Up/Down to select a metric | a to authenticate | q to quit"
    } else {
        "Up/Down to select a metric | q to quit"
    };
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("mbp-info", Style::default().fg(Color::Cyan)),
            Span::raw("  powermetrics --samplers smc"),
        ]),
        Line::from(controls),
    ]);
    frame.render_widget(header, chunks[0]);

    if size.width < 90 {
        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Percentage(40),
                Constraint::Percentage(20),
            ])
            .split(chunks[1]);

        draw_metrics_list(frame, data, app, body_chunks[0]);
        draw_chart(frame, data, app, body_chunks[1]);
        draw_info_panel(frame, data, body_chunks[2]);
    } else {
        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(chunks[1]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(body_chunks[1]);

        draw_metrics_list(frame, data, app, body_chunks[0]);
        draw_chart(frame, data, app, right_chunks[0]);
        draw_info_panel(frame, data, right_chunks[1]);
    }

    let status = build_status_line(data, app);
    let footer = Paragraph::new(Line::from(status)).block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[2]);
}

fn draw_metrics_list(frame: &mut Frame, data: &DataStore, app: &AppState, area: Rect) {
    let entries: Vec<&MetricSeries> = data
        .metric_order()
        .iter()
        .filter_map(|key| data.metrics().get(key))
        .collect();

    let label_width = entries
        .iter()
        .map(|metric| metric.label.len())
        .max()
        .unwrap_or(0)
        .min(40);

    let available_rows = area.height.saturating_sub(2) as usize;
    let total = entries.len();
    let selected = app.selected.min(total.saturating_sub(1));
    let start = if available_rows == 0 || selected < available_rows {
        0
    } else {
        selected + 1 - available_rows
    };
    let end = (start + available_rows).min(total);

    let items: Vec<ListItem> = entries[start..end]
        .iter()
        .map(|metric| {
            let label = format!("{:<width$}", metric.label, width = label_width);
            let latest = metric.values.back().copied();
            let value = latest
                .map(|value| {
                    if metric.unit.is_empty() {
                        format_value(value)
                    } else {
                        format!("{} {}", format_value(value), metric.unit)
                    }
                })
                .unwrap_or_else(|| "-".to_string());
            let line = format!("{label} {value}");
            ListItem::new(Line::from(line))
        })
        .collect();

    let mut state = ListState::default();
    if total > 0 {
        state.select(Some(selected.saturating_sub(start)));
    }

    let list = List::new(items)
        .block(Block::default().title("Metrics").borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_chart(frame: &mut Frame, data: &DataStore, app: &AppState, area: Rect) {
    let Some(selected_key) = app.selected_key(data) else {
        let empty = Paragraph::new("Waiting for powermetrics samples...")
            .block(Block::default().title("Chart").borders(Borders::ALL));
        frame.render_widget(empty, area);
        return;
    };

    let (title, group_series) = group_series_for_selected(data, selected_key);
    if group_series.is_empty() || group_series.iter().all(|series| series.values.is_empty()) {
        let empty = Paragraph::new("No data yet for selected metric.")
            .block(Block::default().title("Chart").borders(Borders::ALL));
        frame.render_widget(empty, area);
        return;
    }

    let mut series_points: Vec<Vec<(f64, f64)>> = Vec::new();
    for series in &group_series {
        let points = series
            .values
            .iter()
            .enumerate()
            .map(|(i, value)| (i as f64, *value))
            .collect();
        series_points.push(points);
    }

    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut max_len = 0usize;
    for series in &group_series {
        max_len = max_len.max(series.values.len());
        for value in &series.values {
            min = min.min(*value);
            max = max.max(*value);
        }
    }

    let padding = if (max - min).abs() < f64::EPSILON {
        if max.abs() < f64::EPSILON {
            1.0
        } else {
            max.abs() * 0.1
        }
    } else {
        (max - min) * 0.1
    };
    let y_min = min - padding;
    let y_max = max + padding;

    let max_x = if max_len > 1 {
        (max_len - 1) as f64
    } else {
        1.0
    };
    let oldest_label = format!(
        "-{}s",
        (max_len.saturating_sub(1) as u64) * SAMPLE_INTERVAL.as_secs()
    );

    let datasets: Vec<Dataset> = group_series
        .iter()
        .enumerate()
        .map(|(index, series)| {
            Dataset::default()
                .name(series.label.clone())
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(CHART_COLORS[index % CHART_COLORS.len()]))
                .data(&series_points[index])
        })
        .collect();

    let chart = Chart::new(datasets)
        .block(Block::default().title(title).borders(Borders::ALL))
        .x_axis(
            Axis::default()
                .title("time")
                .bounds([0.0, max_x])
                .labels(vec![Span::raw(oldest_label), Span::raw("now")]),
        )
        .y_axis(
            Axis::default()
                .title("value")
                .bounds([y_min, y_max])
                .labels(vec![
                    Span::raw(format_value(y_min)),
                    Span::raw(format_value((y_min + y_max) / 2.0)),
                    Span::raw(format_value(y_max)),
                ]),
        );

    frame.render_widget(chart, area);
}

fn draw_info_panel(frame: &mut Frame, data: &DataStore, area: Rect) {
    let max_lines = area.height.saturating_sub(2) as usize;
    let mut lines = Vec::new();

    for label in data.info_order() {
        if let Some(value) = data.info().get(label) {
            lines.push(Line::from(format!("{label}: {value}")));
            if lines.len() >= max_lines {
                break;
            }
        }
    }

    if lines.is_empty() {
        lines.push(Line::from("No static info yet."));
    }

    let panel = Paragraph::new(lines)
        .block(Block::default().title("Static Info").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    frame.render_widget(panel, area);
}

fn build_status_line(data: &DataStore, app: &AppState) -> String {
    if let Some(error) = app.error() {
        return format!("Error: {error}");
    }

    if data.sample_count() == 0 {
        return "Waiting for samples...".to_string();
    }

    let age = data
        .last_sample()
        .map(|instant| instant.elapsed().as_secs())
        .unwrap_or(0);
    format!(
        "Samples: {} | Interval: {}s | Last update: {}s ago",
        data.sample_count(),
        SAMPLE_INTERVAL.as_secs(),
        age
    )
}

fn format_value(value: f64) -> String {
    let fixed = format!("{value:.2}");
    fixed.trim_end_matches('0').trim_end_matches('.').to_string()
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

fn group_series_for_selected<'a>(
    data: &'a DataStore,
    selected_key: &str,
) -> (String, Vec<&'a MetricSeries>) {
    let Some(selected) = data.metrics().get(selected_key) else {
        return ("Chart".to_string(), Vec::new());
    };

    if let Some((group_name, labels)) = group_rule_for_label(&selected.label) {
        let mut series = Vec::new();
        for label in labels {
            if let Some(found) = data.metrics().values().find(|metric| {
                label_matches(&metric.label, label) && metric.unit == selected.unit
            }) {
                series.push(found);
            }
        }
        if !series.is_empty() {
            let title = if selected.unit.is_empty() {
                group_name.to_string()
            } else {
                format!("{group_name} ({})", selected.unit)
            };
            return (title, series);
        }
    }

    let title = if selected.unit.is_empty() {
        selected.label.clone()
    } else {
        format!("{} ({})", selected.label, selected.unit)
    };
    (title, vec![selected])
}
