use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Frame,
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Clear, Dataset, List, ListItem, ListState, Paragraph},
};

use crate::collector::SAMPLE_INTERVAL;
use crate::data::{DashboardStore, DashboardType, DataStore, SeriesValue};
use crate::query::{Query, QueryProvider, QueryResult, QueryRow, QuerySource, SeriesMode};

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
    collectors_count: usize,
    command_mode: bool,
    command_buffer: String,
    command_history: Vec<String>,
    history_index: Option<usize>,
    history_draft: Option<String>,
    completion: Option<CompletionState>,
}

struct Notice {
    title: String,
    rows: Vec<Vec<String>>,
    selected: usize,
    is_error: bool,
    on_enter: Option<NoticeAction>,
}

#[derive(Copy, Clone, Debug)]
pub enum NoticeAction {
    DashboardGoto,
    PrefillCommand,
}

struct CompletionState {
    base: String,
    candidates: Vec<String>,
    index: usize,
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
            collectors_count: 0,
            command_mode: false,
            command_buffer: String::new(),
            command_history: Vec::new(),
            history_index: None,
            history_draft: None,
            completion: None,
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
        let count = data.info_count();
        if count == 0 {
            return;
        }
        self.info_selected = (self.info_selected + 1).min(count - 1);
    }

    pub fn select_info_prev(&mut self, data: &DataStore) {
        if data.info_count() == 0 {
            return;
        }
        self.info_selected = self.info_selected.saturating_sub(1);
    }

    pub fn select_collectors_next(&mut self) {
        let count = self.collectors_count;
        if count == 0 {
            return;
        }
        self.collectors_selected = (self.collectors_selected + 1).min(count - 1);
    }

    pub fn select_collectors_prev(&mut self) {
        if self.collectors_count == 0 {
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

        let info_count = data.info_count();
        if info_count == 0 {
            self.info_selected = 0;
        } else if self.info_selected >= info_count {
            self.info_selected = info_count - 1;
        }

        let collectors_count = self.collectors_count;
        if collectors_count == 0 {
            self.collectors_selected = 0;
        } else if self.collectors_selected >= collectors_count {
            self.collectors_selected = collectors_count - 1;
        }
    }

    pub fn info_selected(&self) -> usize {
        self.info_selected
    }

    pub fn collectors_selected(&self) -> usize {
        self.collectors_selected
    }

    pub fn set_collectors_count(&mut self, count: usize) {
        self.collectors_count = count;
        self.clamp_collectors_selection();
    }

    fn clamp_collectors_selection(&mut self) {
        let count = self.collectors_count;
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
        self.completion = None;
    }

    pub fn exit_command_mode(&mut self) {
        self.command_mode = false;
        self.command_buffer.clear();
        self.history_index = None;
        self.history_draft = None;
        self.completion = None;
    }

    pub fn push_command_char(&mut self, ch: char) {
        self.clear_history_navigation();
        self.clear_completion();
        self.command_buffer.push(ch);
    }

    pub fn pop_command_char(&mut self) {
        self.clear_history_navigation();
        self.clear_completion();
        self.command_buffer.pop();
    }

    pub fn take_command(&mut self) -> String {
        let command = self.command_buffer.trim().to_string();
        self.command_buffer.clear();
        self.history_index = None;
        self.history_draft = None;
        self.completion = None;
        command
    }

    pub fn command_buffer(&self) -> &str {
        &self.command_buffer
    }

    pub fn set_command_buffer(&mut self, value: String) {
        self.clear_completion();
        self.command_buffer = value;
    }

    pub fn clear_command_buffer(&mut self) {
        self.command_buffer.clear();
        self.history_index = None;
        self.history_draft = None;
        self.clear_completion();
    }

    pub fn set_command_buffer_from_completion(&mut self, value: String) {
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

        self.clear_completion();
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

        self.clear_completion();
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
                if data.info_count() > 0 {
                    self.info_selected = 0;
                }
            }
            FocusPanel::Collectors => {
                if self.collectors_count > 0 {
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
                let count = data.info_count();
                if count > 0 {
                    self.info_selected = count - 1;
                }
            }
            FocusPanel::Collectors => {
                let count = self.collectors_count;
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
            on_enter: None,
        });
    }

    pub fn set_notice_with_action(
        &mut self,
        title: &str,
        rows: Vec<Vec<String>>,
        is_error: bool,
        on_enter: NoticeAction,
    ) {
        let selected = if rows.is_empty() { 0 } else { 0 };
        self.notice = Some(Notice {
            title: title.to_string(),
            rows,
            selected,
            is_error,
            on_enter: Some(on_enter),
        });
    }

    pub fn notice_rows(&self) -> Option<&Vec<Vec<String>>> {
        self.notice.as_ref().map(|notice| &notice.rows)
    }

    pub fn notice_title(&self) -> Option<&str> {
        self.notice.as_ref().map(|notice| notice.title.as_str())
    }

    pub fn notice_selected(&self) -> usize {
        self.notice
            .as_ref()
            .map(|notice| notice.selected)
            .unwrap_or(0)
    }

    pub fn notice_is_error(&self) -> bool {
        self.notice
            .as_ref()
            .map(|notice| notice.is_error)
            .unwrap_or(false)
    }

    pub fn notice_action(&self) -> Option<NoticeAction> {
        self.notice.as_ref().and_then(|notice| notice.on_enter)
    }

    pub fn notice_selected_command(&self) -> Option<String> {
        let notice = self.notice.as_ref()?;
        let row = notice.rows.get(notice.selected)?;
        let raw = row.get(0)?.trim();
        if raw.is_empty() {
            return None;
        }
        let value = raw.trim_start_matches(':').trim();
        if value.is_empty() {
            return None;
        }
        let command = value.split_whitespace().next().unwrap_or(value);
        if raw.contains('<') || raw.contains('[') {
            Some(format!("{command} "))
        } else {
            Some(command.to_string())
        }
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

    pub fn set_completion(&mut self, base: String, candidates: Vec<String>) {
        self.completion = Some(CompletionState {
            base,
            candidates,
            index: 0,
        });
    }

    pub fn clear_completion(&mut self) {
        self.completion = None;
    }

    pub fn has_completion(&self) -> bool {
        self.completion.is_some()
    }

    pub fn cycle_completion(&mut self) -> Option<String> {
        let completion = self.completion.as_mut()?;
        if completion.candidates.is_empty() {
            return None;
        }
        completion.index = (completion.index + 1) % completion.candidates.len();
        Some(format!(
            "{}{}",
            completion.base,
            completion.candidates[completion.index]
        ))
    }

    pub fn cycle_completion_prev(&mut self) -> Option<String> {
        let completion = self.completion.as_mut()?;
        if completion.candidates.is_empty() {
            return None;
        }
        if completion.index == 0 {
            completion.index = completion.candidates.len().saturating_sub(1);
        } else {
            completion.index -= 1;
        }
        Some(format!(
            "{}{}",
            completion.base,
            completion.candidates[completion.index]
        ))
    }

    pub fn completion_items(&self) -> Option<(&[String], usize)> {
        let completion = self.completion.as_ref()?;
        Some((&completion.candidates, completion.index))
    }

    fn clear_history_navigation(&mut self) {
        if self.history_index.is_some() {
            self.history_index = None;
            self.history_draft = None;
        }
    }
}

pub fn draw_ui(
    frame: &mut Frame,
    data: &DataStore,
    collectors: &dyn QueryProvider,
    dashboards: &DashboardStore,
    app: &AppState,
) {
    let size = frame.size();
    let areas = panel_areas(size);

    draw_dashboard_bar(frame, dashboards, areas.dashboard);

    match dashboards.active_type() {
        DashboardType::FixedType => {
            draw_metrics_list(
                frame,
                data,
                app,
                app.focus() == FocusPanel::Metrics,
                areas.metrics,
            );
            draw_chart(
                frame,
                data,
                app,
                app.focus() == FocusPanel::Chart,
                areas.chart,
            );
            draw_info_panel(frame, data, app, app.focus() == FocusPanel::Info, areas.info);
            draw_collectors_panel(
                frame,
                collectors,
                app,
                app.focus() == FocusPanel::Collectors,
                areas.collectors,
            );
        }
        DashboardType::BlankType => draw_blank_dashboard(frame, areas.body),
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

    draw_command_bar(frame, app, areas.command);
}

fn draw_dashboard_bar(frame: &mut Frame, dashboards: &DashboardStore, area: Rect) {
    if area.height == 0 {
        return;
    }

    let count = dashboards.count();
    if count == 0 {
        return;
    }

    let active = dashboards.active_index();
    let mut spans = Vec::new();
    for index in 0..count {
        let label = format!("{}", index + 1);
        let style = if index == active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(label, style));
        if index + 1 < count {
            spans.push(Span::raw(" "));
        }
    }

    let bar = Paragraph::new(Line::from(spans));
    frame.render_widget(bar, area);
}

fn draw_blank_dashboard(frame: &mut Frame, area: Rect) {
    if area.height == 0 || area.width == 0 {
        return;
    }

    let block = Block::default().title("Dashboard").borders(Borders::ALL);
    let content = Paragraph::new("Blank dashboard").block(block);
    frame.render_widget(content, area);
}

pub(crate) struct PanelAreas {
    pub dashboard: Rect,
    pub body: Rect,
    pub metrics: Rect,
    pub chart: Rect,
    pub info: Rect,
    pub collectors: Rect,
    pub command: Rect,
}

pub(crate) fn panel_areas(size: Rect) -> PanelAreas {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0), Constraint::Length(1)])
        .split(size);
    let dashboard = chunks[0];
    let body = chunks[1];
    let command = chunks[2];

    if size.width < 90 {
        let body_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(30),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ])
            .split(body);

        return PanelAreas {
            dashboard,
            body,
            metrics: body_chunks[0],
            chart: body_chunks[1],
            info: body_chunks[2],
            collectors: body_chunks[3],
            command,
        };
    }

    let body_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(body);

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(body_chunks[0]);

    let right_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
        .split(body_chunks[1]);

    PanelAreas {
        dashboard,
        body,
        metrics: left_chunks[0],
        collectors: left_chunks[1],
        chart: right_chunks[0],
        info: right_chunks[1],
        command,
    }
}

pub(crate) fn panel_at(size: Rect, x: u16, y: u16, dashboard_type: DashboardType) -> Option<FocusPanel> {
    if matches!(dashboard_type, DashboardType::BlankType) {
        return None;
    }

    let areas = panel_areas(size);
    if point_in_rect(areas.metrics, x, y) {
        return Some(FocusPanel::Metrics);
    }
    if point_in_rect(areas.chart, x, y) {
        return Some(FocusPanel::Chart);
    }
    if point_in_rect(areas.info, x, y) {
        return Some(FocusPanel::Info);
    }
    if point_in_rect(areas.collectors, x, y) {
        return Some(FocusPanel::Collectors);
    }
    None
}

fn point_in_rect(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && x < rect.x.saturating_add(rect.width)
        && y >= rect.y
        && y < rect.y.saturating_add(rect.height)
}

fn draw_metrics_list(
    frame: &mut Frame,
    data: &DataStore,
    app: &AppState,
    focused: bool,
    area: Rect,
) {
    let result = data.query(metrics_table_query(app.selected));
    let rows = metrics_rows(&result, app.selected, focused);
    let table = TuiTable::new(
        result.title,
        result.columns,
        rows,
        app.selected,
        focused,
        "No metrics yet.",
        true,
    );
    draw_tui_table(frame, &table, area);
}

fn draw_chart(frame: &mut Frame, data: &DataStore, app: &AppState, focused: bool, area: Rect) {
    if data.metric_count() == 0 {
        let mut block = Block::default().title("Chart").borders(Borders::ALL);
        if focused {
            block = block.border_style(Style::default().fg(Color::Cyan));
        }
        let empty = Paragraph::new("Waiting for powermetrics samples...").block(block);
        frame.render_widget(empty, area);
        return;
    }

    let chart_result = chart_result_from_query(data.query(metrics_series_query(app.selected)));
    let chart_result = apply_chart_grouping(data, chart_result);

    if chart_result.series.is_empty()
        || chart_result
            .series
            .iter()
            .all(|series| series.points.is_empty())
    {
        let mut block = Block::default().title("Chart").borders(Borders::ALL);
        if focused {
            block = block.border_style(Style::default().fg(Color::Cyan));
        }
        let empty = Paragraph::new("No data yet for selected metric.").block(block);
        frame.render_widget(empty, area);
        return;
    }

    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut max_len = 0usize;
    for series in &chart_result.series {
        max_len = max_len.max(series.points.len());
        for (_, value) in &series.points {
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

    let datasets: Vec<Dataset> = chart_result
        .series
        .iter()
        .enumerate()
        .map(|(index, series)| {
            Dataset::default()
                .name(series.label.clone())
                .marker(symbols::Marker::Braille)
                .style(Style::default().fg(CHART_COLORS[index % CHART_COLORS.len()]))
                .data(&series.points)
        })
        .collect();

    let mut block = Block::default()
        .title(chart_result.title)
        .borders(Borders::ALL);
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

fn draw_info_panel(frame: &mut Frame, data: &DataStore, app: &AppState, focused: bool, area: Rect) {
    let result = data.query(static_info_query(app.info_selected()));
    let rows = rows_from_query(&result);
    let table = TuiTable::new(
        result.title,
        result.columns,
        rows,
        app.info_selected(),
        focused,
        "No static info yet.",
        false,
    );
    draw_tui_table(frame, &table, area);
}

fn draw_collectors_panel(
    frame: &mut Frame,
    collectors: &dyn QueryProvider,
    app: &AppState,
    focused: bool,
    area: Rect,
) {
    let result = collectors.query(collectors_table_query(app.collectors_selected()));
    let rows = rows_from_query(&result);
    let table = TuiTable::new(
        result.title,
        result.columns,
        rows,
        app.collectors_selected(),
        focused,
        "No collectors registered.",
        true,
    );
    draw_tui_table(frame, &table, area);
}

fn draw_table_with_style(
    frame: &mut Frame,
    columns: &[String],
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

    let column_count = rows
        .iter()
        .map(|row| row.len())
        .max()
        .unwrap_or(1)
        .max(columns.len().max(1));
    let right_align = right_align_last && column_count > 1;
    let mut widths = vec![1usize; column_count];
    for row in &rows {
        for (index, col) in row.iter().enumerate() {
            widths[index] = widths[index].max(col.len().max(1));
        }
    }
    for (index, col) in columns.iter().enumerate() {
        widths[index] = widths[index].max(col.len().max(1));
    }
    if let Some(first_width) = widths.get_mut(0) {
        *first_width = (*first_width).min(40);
    }

    let inner_height = area.height.saturating_sub(2);
    let show_header = !columns.is_empty() && inner_height > 0;
    let header_height = if show_header { 1 } else { 0 };
    let available_rows = inner_height.saturating_sub(header_height) as usize;
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
            let mut parts = Vec::with_capacity(column_count);
            for index in 0..column_count {
                let value = row.get(index).map(String::as_str).unwrap_or("");
                let width = widths.get(index).copied().unwrap_or(1);
                let formatted = if right_align && index + 1 == column_count {
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
    frame.render_widget(block, area);

    let inner = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: inner_height,
    };
    let (header_area, list_area) = if show_header {
        (
            Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: 1,
            },
            Rect {
                x: inner.x,
                y: inner.y.saturating_add(1),
                width: inner.width,
                height: inner.height.saturating_sub(1),
            },
        )
    } else {
        (
            Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: 0,
            },
            inner,
        )
    };

    let highlight_symbol = ">> ";
    if show_header {
        let mut parts = Vec::with_capacity(column_count);
        for index in 0..column_count {
            let value = columns.get(index).map(String::as_str).unwrap_or("");
            let width = widths.get(index).copied().unwrap_or(1);
            let formatted = if right_align && index + 1 == column_count {
                format!("{value:>width$}")
            } else {
                format!("{value:<width$}")
            };
            parts.push(formatted);
        }
        let prefix = " ".repeat(highlight_symbol.chars().count());
        let header = Paragraph::new(Line::from(format!("{prefix}{}", parts.join(" "))))
            .style(Style::default().add_modifier(Modifier::BOLD));
        frame.render_widget(header, header_area);
    }

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(highlight_symbol);

    frame.render_stateful_widget(list, list_area, &mut state);
}

struct TuiTable {
    title: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    selected: usize,
    focused: bool,
    empty_text: String,
    right_align_last: bool,
    border_style: Option<Style>,
}

impl TuiTable {
    fn new(
        title: String,
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        selected: usize,
        focused: bool,
        empty_text: &str,
        right_align_last: bool,
    ) -> Self {
        Self {
            title,
            columns,
            rows,
            selected,
            focused,
            empty_text: empty_text.to_string(),
            right_align_last,
            border_style: None,
        }
    }

    fn with_border_style(mut self, style: Option<Style>) -> Self {
        self.border_style = style;
        self
    }

}

fn draw_tui_table(frame: &mut Frame, table: &TuiTable, area: Rect) {
    draw_table_with_style(
        frame,
        &table.columns,
        table.rows.clone(),
        area,
        table.focused,
        table.selected,
        &table.title,
        &table.empty_text,
        table.right_align_last,
        table.border_style,
    );
}

fn draw_command_bar(frame: &mut Frame, app: &AppState, area: Rect) {
    let command = if app.is_command_mode() {
        let mut spans = Vec::new();
        spans.push(Span::raw(format!(":{}", app.command_buffer())));
        if let Some((items, index)) = app.completion_items() {
            spans.push(Span::raw("  "));
            for (item_index, item) in items.iter().enumerate() {
                let style = if item_index == index {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                spans.push(Span::styled(item.clone(), style));
                if item_index + 1 < items.len() {
                    spans.push(Span::raw(" "));
                }
            }
        }
        Paragraph::new(Line::from(spans))
    } else {
        Paragraph::new(Line::from(String::new()))
    };
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
    let popup_area = Rect {
        x,
        y,
        width,
        height,
    };

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

    let table = TuiTable::new(
        title.to_string(),
        Vec::new(),
        temp_rows,
        selected,
        true,
        "No output.",
        false,
    )
    .with_border_style(border_style);
    draw_tui_table(frame, &table, popup_area);
}

#[derive(Clone)]
struct ChartSeries {
    label: String,
    unit: String,
    points: Vec<(f64, f64)>,
}

struct ChartResult {
    title: String,
    series: Vec<ChartSeries>,
}

fn chart_result_from_query(result: QueryResult) -> ChartResult {
    let series = match result.series_column {
        Some(index) => result
            .rows
            .iter()
            .filter_map(|row| chart_series_from_row(row, index))
            .collect(),
        None => Vec::new(),
    };

    ChartResult {
        title: result.title,
        series,
    }
}

fn chart_series_from_row(row: &QueryRow, series_column: usize) -> Option<ChartSeries> {
    let values = row.cells.get(series_column)?;
    let mut points = Vec::new();
    for (index, value) in values.iter().enumerate() {
        if let SeriesValue::Number(number) = value {
            points.push((index as f64, *number));
        }
    }

    let label = row
        .series_label
        .clone()
        .or_else(|| row.cells.first().and_then(cell_label))
        .unwrap_or_else(|| "Series".to_string());
    let unit = row.series_unit.clone().unwrap_or_default();

    Some(ChartSeries {
        label,
        unit,
        points,
    })
}

fn apply_chart_grouping(data: &DataStore, base: ChartResult) -> ChartResult {
    let Some(selected) = base.series.first() else {
        return base;
    };

    let Some((group_name, labels)) = group_rule_for_label(&selected.label) else {
        return base;
    };

    let all_series = chart_result_from_query(data.query(metrics_series_all_query()));

    let mut grouped = Vec::new();
    for label in labels {
        if let Some(found) = all_series
            .series
            .iter()
            .find(|series| label_matches(&series.label, label) && series.unit == selected.unit)
        {
            grouped.push(found.clone());
        }
    }

    if grouped.is_empty() {
        return base;
    }

    let title = if selected.unit.is_empty() {
        group_name.to_string()
    } else {
        format!("{group_name} ({})", selected.unit)
    };

    ChartResult {
        title,
        series: grouped,
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

fn metrics_table_query(selected: usize) -> Query {
    Query {
        source: QuerySource::Metrics,
        selected_index: Some(selected),
        mode: SeriesMode::Latest,
    }
}

fn static_info_query(selected: usize) -> Query {
    Query {
        source: QuerySource::StaticInfo,
        selected_index: Some(selected),
        mode: SeriesMode::Latest,
    }
}

fn collectors_table_query(selected: usize) -> Query {
    Query {
        source: QuerySource::Collectors,
        selected_index: Some(selected),
        mode: SeriesMode::Latest,
    }
}

fn metrics_series_query(selected: usize) -> Query {
    Query {
        source: QuerySource::Metrics,
        selected_index: Some(selected),
        mode: SeriesMode::Full,
    }
}

fn metrics_series_all_query() -> Query {
    Query {
        source: QuerySource::Metrics,
        selected_index: None,
        mode: SeriesMode::Full,
    }
}

fn rows_from_text(text: &str) -> Vec<Vec<String>> {
    text.lines().map(|line| vec![line.to_string()]).collect()
}

pub(crate) fn rows_from_query(result: &QueryResult) -> Vec<Vec<String>> {
    result
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    let unit = if result.series_column == Some(index) {
                        row.series_unit.as_deref().unwrap_or("")
                    } else {
                        ""
                    };
                    format_latest_cell(cell, unit)
                })
                .collect()
        })
        .collect()
}

fn metrics_rows(result: &QueryResult, selected: usize, show_samples: bool) -> Vec<Vec<String>> {
    let sample_column = result.columns.len().saturating_sub(1);
    result
        .rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let mut values = Vec::with_capacity(row.cells.len());
            for (col_index, cell) in row.cells.iter().enumerate() {
                if col_index == sample_column {
                    if show_samples && index == selected {
                        values.push(format_sample_cell(row));
                    } else {
                        values.push(String::new());
                    }
                    continue;
                }

                let unit = if result.series_column == Some(col_index) {
                    row.series_unit.as_deref().unwrap_or("")
                } else {
                    ""
                };
                values.push(format_latest_cell(cell, unit));
            }
            values
        })
        .collect()
}

fn format_sample_cell(row: &QueryRow) -> String {
    match row.sample_count {
        Some(count) => sample_label(count),
        None => String::new(),
    }
}

fn sample_label(count: u64) -> String {
    if count == 1 {
        "1 sample".to_string()
    } else {
        format!("{count} samples")
    }
}

fn format_latest_cell(values: &[SeriesValue], unit: &str) -> String {
    match values.last() {
        Some(SeriesValue::Number(value)) => {
            if unit.is_empty() {
                format_number(*value)
            } else {
                format!("{} {}", format_number(*value), unit)
            }
        }
        Some(SeriesValue::Text(value)) => value.clone(),
        None => "-".to_string(),
    }
}

fn format_number(value: f64) -> String {
    let fixed = format!("{value:.2}");
    fixed
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

fn cell_label(cell: &Vec<SeriesValue>) -> Option<String> {
    cell.last().map(|value| match value {
        SeriesValue::Number(value) => format_number(*value),
        SeriesValue::Text(value) => value.clone(),
    })
}

fn format_value(value: f64) -> String {
    format_number(value)
}
