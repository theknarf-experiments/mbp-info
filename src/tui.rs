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
    message: Option<String>,
    focus: FocusPanel,
    command_mode: bool,
    command_buffer: String,
    focus_pending: bool,
    command_history: Vec<String>,
    history_index: Option<usize>,
    history_draft: Option<String>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FocusPanel {
    Metrics,
    Chart,
    Info,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            selected: 0,
            error: None,
            message: None,
            focus: FocusPanel::Metrics,
            command_mode: false,
            command_buffer: String::new(),
            focus_pending: false,
            command_history: Vec::new(),
            history_index: None,
            history_draft: None,
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

    pub fn is_command_mode(&self) -> bool {
        self.command_mode
    }

    pub fn enter_command_mode(&mut self) {
        self.command_mode = true;
        self.command_buffer.clear();
        self.history_index = None;
        self.history_draft = None;
    }

    pub fn exit_command_mode(&mut self) {
        self.command_mode = false;
        self.command_buffer.clear();
        self.history_index = None;
        self.history_draft = None;
    }

    pub fn push_command_char(&mut self, ch: char) {
        self.clear_history_navigation();
        self.command_buffer.push(ch);
    }

    pub fn pop_command_char(&mut self) {
        self.clear_history_navigation();
        self.command_buffer.pop();
    }

    pub fn take_command(&mut self) -> String {
        let command = self.command_buffer.trim().to_string();
        self.command_buffer.clear();
        self.history_index = None;
        self.history_draft = None;
        command
    }

    pub fn command_buffer(&self) -> &str {
        &self.command_buffer
    }

    pub fn set_command_buffer(&mut self, value: String) {
        self.command_buffer = value;
    }

    pub fn record_command(&mut self, command: &str) {
        if !command.is_empty() {
            self.command_history.push(command.to_string());
        }
        self.history_index = None;
        self.history_draft = None;
    }

    pub fn history_prev(&mut self) {
        if self.command_history.is_empty() {
            return;
        }

        let next_index = match self.history_index {
            Some(index) => index.saturating_sub(1),
            None => {
                self.history_draft = Some(self.command_buffer.clone());
                self.command_history.len().saturating_sub(1)
            }
        };

        self.history_index = Some(next_index);
        if let Some(command) = self.command_history.get(next_index) {
            self.command_buffer = command.clone();
        }
    }

    pub fn history_next(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };

        if index + 1 < self.command_history.len() {
            let next_index = index + 1;
            self.history_index = Some(next_index);
            if let Some(command) = self.command_history.get(next_index) {
                self.command_buffer = command.clone();
            }
        } else {
            self.history_index = None;
            self.command_buffer = self.history_draft.take().unwrap_or_default();
        }
    }

    pub fn focus(&self) -> FocusPanel {
        self.focus
    }

    pub fn set_focus(&mut self, focus: FocusPanel) {
        self.focus = focus;
    }

    pub fn focus_pending(&self) -> bool {
        self.focus_pending
    }

    pub fn set_focus_pending(&mut self, pending: bool) {
        self.focus_pending = pending;
    }

    pub fn set_error(&mut self, error: Option<String>) {
        self.error = error;
        if self.error.is_some() {
            self.message = None;
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn set_message(&mut self, message: Option<String>) {
        self.message = message;
        if self.message.is_some() {
            self.error = None;
        }
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    fn clear_history_navigation(&mut self) {
        if self.history_index.is_some() {
            self.history_index = None;
            self.history_draft = None;
        }
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
            Constraint::Length(1),
        ])
        .split(size);

    let controls = "Arrows/jk to select | : for commands | Ctrl+w then hjkl/arrow to focus";
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

        draw_metrics_list(
            frame,
            data,
            app,
            app.focus() == FocusPanel::Metrics,
            body_chunks[0],
        );
        draw_chart(
            frame,
            data,
            app,
            app.focus() == FocusPanel::Chart,
            body_chunks[1],
        );
        draw_info_panel(
            frame,
            data,
            app.focus() == FocusPanel::Info,
            body_chunks[2],
        );
    } else {
        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(chunks[1]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(body_chunks[1]);

        draw_metrics_list(
            frame,
            data,
            app,
            app.focus() == FocusPanel::Metrics,
            body_chunks[0],
        );
        draw_chart(
            frame,
            data,
            app,
            app.focus() == FocusPanel::Chart,
            right_chunks[0],
        );
        draw_info_panel(
            frame,
            data,
            app.focus() == FocusPanel::Info,
            right_chunks[1],
        );
    }

    let status = build_status_line(data, app);
    let footer =
        Paragraph::new(Line::from(status)).block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[2]);

    draw_command_bar(frame, app, chunks[3]);
}

fn draw_metrics_list(
    frame: &mut Frame,
    data: &DataStore,
    app: &AppState,
    focused: bool,
    area: Rect,
) {
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

    let mut block = Block::default().title("Metrics").borders(Borders::ALL);
    if focused {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }
    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_chart(
    frame: &mut Frame,
    data: &DataStore,
    app: &AppState,
    focused: bool,
    area: Rect,
) {
    let Some(selected_key) = app.selected_key(data) else {
        let mut block = Block::default().title("Chart").borders(Borders::ALL);
        if focused {
            block = block.border_style(Style::default().fg(Color::Cyan));
        }
        let empty = Paragraph::new("Waiting for powermetrics samples...").block(block);
        frame.render_widget(empty, area);
        return;
    };

    let (title, group_series) = group_series_for_selected(data, selected_key);
    if group_series.is_empty() || group_series.iter().all(|series| series.values.is_empty()) {
        let mut block = Block::default().title("Chart").borders(Borders::ALL);
        if focused {
            block = block.border_style(Style::default().fg(Color::Cyan));
        }
        let empty = Paragraph::new("No data yet for selected metric.").block(block);
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

    let mut block = Block::default().title(title).borders(Borders::ALL);
    if focused {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }
    let chart = Chart::new(datasets)
        .block(block)
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

fn draw_info_panel(frame: &mut Frame, data: &DataStore, focused: bool, area: Rect) {
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

    let mut block = Block::default().title("Static Info").borders(Borders::ALL);
    if focused {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }
    let panel = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: true });
    frame.render_widget(panel, area);
}

fn draw_command_bar(frame: &mut Frame, app: &AppState, area: Rect) {
    let content = if app.is_command_mode() {
        format!(":{}", app.command_buffer())
    } else {
        String::new()
    };
    let command = Paragraph::new(Line::from(content));
    frame.render_widget(command, area);
}

fn build_status_line(data: &DataStore, app: &AppState) -> String {
    if let Some(error) = app.error() {
        return format!("Error: {error}");
    }

    if let Some(message) = app.message() {
        return message.to_string();
    }

    if data.sample_count() == 0 {
        return "Waiting for samples...".to_string();
    }

    let age = data
        .last_sample()
        .map(|instant| instant.elapsed().as_secs())
        .unwrap_or(0);
    let focus = match app.focus() {
        FocusPanel::Metrics => "Metrics",
        FocusPanel::Chart => "Chart",
        FocusPanel::Info => "Info",
    };
    format!(
        "Samples: {} | Interval: {}s | Last update: {}s ago | Focus: {}",
        data.sample_count(),
        SAMPLE_INTERVAL.as_secs(),
        age,
        focus
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
