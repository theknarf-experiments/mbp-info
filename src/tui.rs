use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Frame,
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Clear, Dataset, List, ListItem, ListState, Paragraph},
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

static GROUP_RULES: &[(&str, &[&str])] = &[(
    "Die temperature",
    &["CPU die temperature", "GPU die temperature"],
)];

pub struct AppState {
    selected: usize,
    notice: Option<Notice>,
    focus: FocusPanel,
    info_selected: usize,
    collectors_selected: usize,
    collectors_rows: Vec<Vec<String>>,
    command_mode: bool,
    command_buffer: String,
    command_history: Vec<String>,
    history_index: Option<usize>,
    history_draft: Option<String>,
}

struct Notice {
    title: String,
    rows: Vec<Vec<String>>,
    selected: usize,
    is_error: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FocusPanel {
    Metrics,
    Chart,
    Info,
    Collectors,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            selected: 0,
            notice: None,
            focus: FocusPanel::Metrics,
            info_selected: 0,
            collectors_selected: 0,
            collectors_rows: Vec::new(),
            command_mode: false,
            command_buffer: String::new(),
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

    pub fn select_info_next(&mut self, data: &DataStore) {
        let count = data.info_order().len();
        if count == 0 {
            return;
        }
        self.info_selected = (self.info_selected + 1).min(count - 1);
    }

    pub fn select_info_prev(&mut self, data: &DataStore) {
        if data.info_order().is_empty() {
            return;
        }
        self.info_selected = self.info_selected.saturating_sub(1);
    }

    pub fn select_collectors_next(&mut self) {
        let count = self.collectors_rows.len();
        if count == 0 {
            return;
        }
        self.collectors_selected = (self.collectors_selected + 1).min(count - 1);
    }

    pub fn select_collectors_prev(&mut self) {
        if self.collectors_rows.is_empty() {
            return;
        }
        self.collectors_selected = self.collectors_selected.saturating_sub(1);
    }

    pub fn clamp_selection(&mut self, data: &DataStore) {
        let count = data.metric_count();
        if count == 0 {
            self.selected = 0;
        } else if self.selected >= count {
            self.selected = count - 1;
        }

        let info_count = data.info_order().len();
        if info_count == 0 {
            self.info_selected = 0;
        } else if self.info_selected >= info_count {
            self.info_selected = info_count - 1;
        }

        let collectors_count = self.collectors_rows.len();
        if collectors_count == 0 {
            self.collectors_selected = 0;
        } else if self.collectors_selected >= collectors_count {
            self.collectors_selected = collectors_count - 1;
        }
    }

    pub fn selected_key<'a>(&self, data: &'a DataStore) -> Option<&'a str> {
        data.metric_key(self.selected)
    }

    pub fn info_selected(&self) -> usize {
        self.info_selected
    }

    pub fn collectors_selected(&self) -> usize {
        self.collectors_selected
    }

    pub fn set_collectors_rows(&mut self, rows: Vec<Vec<String>>) {
        self.collectors_rows = rows;
        self.clamp_collectors_selection();
    }

    pub fn collectors_rows(&self) -> &Vec<Vec<String>> {
        &self.collectors_rows
    }

    fn clamp_collectors_selection(&mut self) {
        let count = self.collectors_rows.len();
        if count == 0 {
            self.collectors_selected = 0;
        } else if self.collectors_selected >= count {
            self.collectors_selected = count - 1;
        }
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

    pub fn jump_top(&mut self, data: &DataStore) {
        match self.focus {
            FocusPanel::Metrics => {
                if data.metric_count() > 0 {
                    self.selected = 0;
                }
            }
            FocusPanel::Info => {
                if !data.info_order().is_empty() {
                    self.info_selected = 0;
                }
            }
            FocusPanel::Collectors => {
                if !self.collectors_rows.is_empty() {
                    self.collectors_selected = 0;
                }
            }
            FocusPanel::Chart => {}
        }
    }

    pub fn jump_bottom(&mut self, data: &DataStore) {
        match self.focus {
            FocusPanel::Metrics => {
                let count = data.metric_count();
                if count > 0 {
                    self.selected = count - 1;
                }
            }
            FocusPanel::Info => {
                let count = data.info_order().len();
                if count > 0 {
                    self.info_selected = count - 1;
                }
            }
            FocusPanel::Collectors => {
                let count = self.collectors_rows.len();
                if count > 0 {
                    self.collectors_selected = count - 1;
                }
            }
            FocusPanel::Chart => {}
        }
    }

    pub fn set_error(&mut self, error: Option<String>) {
        match error {
            Some(error) => self.set_notice("Error", rows_from_text(&error), true),
            None => self.clear_notice(),
        }
    }

    pub fn set_message(&mut self, message: Option<String>) {
        match message {
            Some(message) => self.set_notice("Notice", rows_from_text(&message), false),
            None => self.clear_notice(),
        }
    }

    pub fn set_notice(&mut self, title: &str, rows: Vec<Vec<String>>, is_error: bool) {
        let selected = if rows.is_empty() { 0 } else { 0 };
        self.notice = Some(Notice {
            title: title.to_string(),
            rows,
            selected,
            is_error,
        });
    }

    pub fn notice_rows(&self) -> Option<&Vec<Vec<String>>> {
        self.notice.as_ref().map(|notice| &notice.rows)
    }

    pub fn notice_title(&self) -> Option<&str> {
        self.notice.as_ref().map(|notice| notice.title.as_str())
    }

    pub fn notice_selected(&self) -> usize {
        self.notice.as_ref().map(|notice| notice.selected).unwrap_or(0)
    }

    pub fn notice_is_error(&self) -> bool {
        self.notice.as_ref().map(|notice| notice.is_error).unwrap_or(false)
    }

    pub fn select_notice_next(&mut self) {
        let Some(notice) = self.notice.as_mut() else {
            return;
        };
        if notice.rows.is_empty() {
            return;
        }
        notice.selected = (notice.selected + 1).min(notice.rows.len() - 1);
    }

    pub fn select_notice_prev(&mut self) {
        let Some(notice) = self.notice.as_mut() else {
            return;
        };
        if notice.rows.is_empty() {
            return;
        }
        notice.selected = notice.selected.saturating_sub(1);
    }

    pub fn jump_notice_top(&mut self) {
        if let Some(notice) = self.notice.as_mut() {
            if !notice.rows.is_empty() {
                notice.selected = 0;
            }
        }
    }

    pub fn jump_notice_bottom(&mut self) {
        if let Some(notice) = self.notice.as_mut() {
            let count = notice.rows.len();
            if count > 0 {
                notice.selected = count - 1;
            }
        }
    }

    pub fn has_notice(&self) -> bool {
        self.notice.is_some()
    }

    pub fn clear_notice(&mut self) {
        self.notice = None;
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
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(size);

    if size.width < 90 {
        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(30),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ])
            .split(chunks[0]);

        let metrics_area = body_chunks[0];
        let chart_area = body_chunks[1];
        let info_area = body_chunks[2];
        let collectors_area = body_chunks[3];

        draw_metrics_list(frame, data, app, app.focus() == FocusPanel::Metrics, metrics_area);
        draw_chart(
            frame,
            data,
            app,
            app.focus() == FocusPanel::Chart,
            chart_area,
        );
        draw_info_panel(
            frame,
            data,
            app,
            app.focus() == FocusPanel::Info,
            info_area,
        );
        draw_collectors_panel(
            frame,
            app,
            app.focus() == FocusPanel::Collectors,
            collectors_area,
        );
    } else {
        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(chunks[0]);

        let left_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(body_chunks[0]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(body_chunks[1]);

        let metrics_area = left_chunks[0];
        let collectors_area = left_chunks[1];
        let chart_area = right_chunks[0];
        let info_area = right_chunks[1];

        draw_metrics_list(frame, data, app, app.focus() == FocusPanel::Metrics, metrics_area);
        draw_collectors_panel(
            frame,
            app,
            app.focus() == FocusPanel::Collectors,
            collectors_area,
        );
        draw_chart(
            frame,
            data,
            app,
            app.focus() == FocusPanel::Chart,
            chart_area,
        );
        draw_info_panel(
            frame,
            data,
            app,
            app.focus() == FocusPanel::Info,
            info_area,
        );
    }

    if let Some(rows) = app.notice_rows() {
        if let Some(title) = app.notice_title() {
            draw_notice_popup(
                frame,
                size,
                title,
                rows,
                app.notice_selected(),
                app.notice_is_error(),
            );
        }
    }

    draw_command_bar(frame, app, chunks[1]);
}

fn draw_metrics_list(
    frame: &mut Frame,
    data: &DataStore,
    app: &AppState,
    focused: bool,
    area: Rect,
) {
    let entries: Vec<(&str, &MetricSeries)> = data
        .metric_order()
        .iter()
        .filter_map(|key| data.metrics().get(key).map(|metric| (key.as_str(), metric)))
        .collect();

    let value_strings: Vec<String> = entries
        .iter()
        .map(|(_, metric)| {
            let latest = metric.values.back().copied();
            latest
                .map(|value| {
                    if metric.unit.is_empty() {
                        format_value(value)
                    } else {
                        format!("{} {}", format_value(value), metric.unit)
                    }
                })
                .unwrap_or_else(|| "-".to_string())
        })
        .collect();

    let sample_strings: Vec<String> = entries
        .iter()
        .map(|(key, _)| sample_label(data.metric_sample_count(key)))
        .collect();

    let rows: Vec<Vec<String>> = entries
        .iter()
        .enumerate()
        .map(|(index, (_, metric))| {
            let samples = if focused && index == app.selected {
                sample_strings[index].clone()
            } else {
                String::new()
            };
            vec![
                metric.label.clone(),
                value_strings[index].clone(),
                samples,
            ]
        })
        .collect();

    let selected = app.selected.min(rows.len().saturating_sub(1));
    draw_table(
        frame,
        rows,
        area,
        focused,
        selected,
        "Metrics",
        "No metrics yet.",
        true,
    );
}

fn draw_chart(frame: &mut Frame, data: &DataStore, app: &AppState, focused: bool, area: Rect) {
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

fn draw_info_panel(
    frame: &mut Frame,
    data: &DataStore,
    app: &AppState,
    focused: bool,
    area: Rect,
) {
    let rows: Vec<Vec<String>> = data
        .info_order()
        .iter()
        .filter_map(|label| {
            data.info()
                .get(label)
                .map(|value| vec![label.clone(), value.clone()])
        })
        .collect();

    let selected = app.info_selected().min(rows.len().saturating_sub(1));
    draw_table(
        frame,
        rows,
        area,
        focused,
        selected,
        "Static Info",
        "No static info yet.",
        false,
    );
}

fn draw_collectors_panel(frame: &mut Frame, app: &AppState, focused: bool, area: Rect) {
    let rows = app.collectors_rows().clone();
    let selected = app.collectors_selected().min(rows.len().saturating_sub(1));
    draw_table(
        frame,
        rows,
        area,
        focused,
        selected,
        "Collectors",
        "No collectors registered.",
        true,
    );
}

fn draw_table(
    frame: &mut Frame,
    rows: Vec<Vec<String>>,
    area: Rect,
    focused: bool,
    selected: usize,
    title: &str,
    empty_text: &str,
    right_align_last: bool,
) {
    draw_table_with_style(
        frame,
        rows,
        area,
        focused,
        selected,
        title,
        empty_text,
        right_align_last,
        None,
    );
}

fn draw_table_with_style(
    frame: &mut Frame,
    mut rows: Vec<Vec<String>>,
    area: Rect,
    focused: bool,
    selected: usize,
    title: &str,
    empty_text: &str,
    right_align_last: bool,
    border_style: Option<Style>,
) {
    if rows.is_empty() {
        rows.push(vec![empty_text.to_string()]);
    }

    let columns = rows.iter().map(|row| row.len()).max().unwrap_or(1);
    let right_align = right_align_last && columns > 1;
    let mut widths = vec![1usize; columns];
    for row in &rows {
        for (index, col) in row.iter().enumerate() {
            widths[index] = widths[index].max(col.len().max(1));
        }
    }
    if let Some(first_width) = widths.get_mut(0) {
        *first_width = (*first_width).min(40);
    }

    let available_rows = area.height.saturating_sub(2) as usize;
    let total = rows.len();
    let selected = selected.min(total.saturating_sub(1));
    let start = if available_rows == 0 || selected < available_rows {
        0
    } else {
        selected + 1 - available_rows
    };
    let end = (start + available_rows).min(total);

    let items: Vec<ListItem> = rows
        .iter()
        .skip(start)
        .take(end - start)
        .map(|row| {
            let mut parts = Vec::with_capacity(columns);
            for index in 0..columns {
                let value = row.get(index).map(String::as_str).unwrap_or("");
                let width = widths.get(index).copied().unwrap_or(1);
                let formatted = if right_align && index + 1 == columns {
                    format!("{value:>width$}")
                } else {
                    format!("{value:<width$}")
                };
                parts.push(formatted);
            }
            ListItem::new(Line::from(parts.join(" ")))
        })
        .collect();

    let mut state = ListState::default();
    if total > 0 {
        state.select(Some(selected.saturating_sub(start)));
    }

    let mut block = Block::default().title(title).borders(Borders::ALL);
    if focused {
        let style = border_style.unwrap_or_else(|| Style::default().fg(Color::Cyan));
        block = block.border_style(style);
    } else if let Some(style) = border_style {
        block = block.border_style(style);
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

fn draw_command_bar(frame: &mut Frame, app: &AppState, area: Rect) {
    let content = if app.is_command_mode() {
        format!(":{}", app.command_buffer())
    } else {
        String::new()
    };
    let command = Paragraph::new(Line::from(content));
    frame.render_widget(command, area);
}

fn draw_notice_popup(
    frame: &mut Frame,
    size: Rect,
    title: &str,
    rows: &Vec<Vec<String>>,
    selected: usize,
    is_error: bool,
) {
    let max_width = size.width.saturating_sub(4);
    let max_height = size.height.saturating_sub(4);
    if max_width < 20 || max_height < 6 {
        return;
    }

    let width = ((size.width as f32) * 0.7) as u16;
    let height = ((size.height as f32) * 0.6) as u16;
    let width = width.max(40).min(max_width);
    let height = height.max(10).min(max_height);

    let x = size.x + (size.width.saturating_sub(width)) / 2;
    let y = size.y + (size.height.saturating_sub(height)) / 2;
    let popup_area = Rect { x, y, width, height };

    frame.render_widget(Clear, popup_area);

    let mut temp_rows = rows.clone();
    if temp_rows.is_empty() {
        temp_rows.push(vec!["No output.".to_string()]);
    }

    let mut selected = selected.min(temp_rows.len().saturating_sub(1));
    if temp_rows.is_empty() {
        selected = 0;
    }

    let border_style = if is_error {
        Some(Style::default().fg(Color::LightRed))
    } else {
        None
    };

    draw_table_with_style(
        frame,
        temp_rows,
        popup_area,
        true,
        selected,
        title,
        "No output.",
        false,
        border_style,
    );
}

fn rows_from_text(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .map(|line| vec![line.to_string()])
        .collect()
}

fn format_value(value: f64) -> String {
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
            if let Some(found) = data
                .metrics()
                .values()
                .find(|metric| label_matches(&metric.label, label) && metric.unit == selected.unit)
            {
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
