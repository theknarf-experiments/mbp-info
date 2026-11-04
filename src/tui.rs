use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use crate::data::{DashboardStore, DashboardType, DataStore, SeriesValue};
use crate::query::{
    Query, QueryContext, QueryProvider, QueryResult, SeriesKind, SeriesSelector, TimeRange,
};
use crate::widget::{draw_widget_grid, draw_widget_result, TableOptions, Widget, WidgetView};

/// Metrics charted together on the fixed dashboard when one of them is selected.
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
        self.notice = Some(Notice {
            title: title.to_string(),
            rows,
            selected: 0,
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
        self.notice = Some(Notice {
            title: title.to_string(),
            rows,
            selected: 0,
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
        let raw = row.first()?.trim();
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
            completion.base, completion.candidates[completion.index]
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
            completion.base, completion.candidates[completion.index]
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

    let ctx = QueryContext {
        data,
        collectors,
        dashboards,
    };
    match dashboards.active() {
        Some(dashboard) if matches!(dashboard.kind, DashboardType::BlankType) => {
            draw_widget_grid(frame, &dashboard.widgets, &ctx, areas.body)
        }
        _ => draw_fixed_dashboard(frame, &ctx, app, &areas),
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
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
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

pub(crate) fn panel_at(
    size: Rect,
    x: u16,
    y: u16,
    dashboard_type: DashboardType,
) -> Option<FocusPanel> {
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

pub(crate) struct TuiTable {
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
    pub(crate) fn new(
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

pub(crate) fn draw_tui_table(frame: &mut Frame, table: &TuiTable, area: Rect) {
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
    rows: &[Vec<String>],
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

    let mut temp_rows = rows.to_vec();
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

fn draw_fixed_dashboard(frame: &mut Frame, ctx: &QueryContext, app: &AppState, areas: &PanelAreas) {
    let metrics = Widget::table(
        Query::series(SeriesSelector::kind(SeriesKind::Metric), TimeRange::Latest),
        TableOptions {
            show_samples: true,
            right_align_last: true,
        },
    )
    .with_empty_text("No metrics yet.");
    let info = Widget::table(
        Query::series(SeriesSelector::kind(SeriesKind::Info), TimeRange::Latest),
        TableOptions::default(),
    )
    .with_empty_text("No static info yet.");
    let collectors = Widget::table(
        Query::collectors(),
        TableOptions {
            show_samples: false,
            right_align_last: true,
        },
    )
    .with_empty_text("No collectors registered.");

    // The chart follows the metrics table selection, so its query is built
    // from the selected row rather than reading screen state itself.
    let metrics_result = ctx.query(&metrics.query);
    let chart = chart_for_selection(&metrics_result, app.selected);

    let view = |panel: FocusPanel, selected: usize| WidgetView {
        focused: app.focus() == panel,
        selected,
    };
    draw_widget_result(
        frame,
        &metrics,
        &metrics_result,
        view(FocusPanel::Metrics, app.selected),
        areas.metrics,
    );
    draw_widget_result(
        frame,
        &chart,
        &ctx.query(&chart.query),
        view(FocusPanel::Chart, 0),
        areas.chart,
    );
    draw_widget_result(
        frame,
        &info,
        &ctx.query(&info.query),
        view(FocusPanel::Info, app.info_selected()),
        areas.info,
    );
    draw_widget_result(
        frame,
        &collectors,
        &ctx.query(&collectors.query),
        view(FocusPanel::Collectors, app.collectors_selected()),
        areas.collectors,
    );
}

fn chart_for_selection(metrics: &QueryResult, selected: usize) -> Widget {
    let all_series: Vec<_> = metrics
        .rows
        .iter()
        .filter_map(|row| row.series.as_ref())
        .collect();
    let Some(selected) = all_series.get(selected) else {
        return Widget::chart(Query::series(
            SeriesSelector::ids(Vec::new()),
            TimeRange::All,
        ))
        .with_title("Chart");
    };

    if let Some((group_name, labels)) = group_rule_for_label(&selected.label) {
        let ids: Vec<String> = labels
            .iter()
            .filter_map(|label| {
                all_series
                    .iter()
                    .find(|series| {
                        label_matches(&series.label, label) && series.unit == selected.unit
                    })
                    .map(|series| series.id.clone())
            })
            .collect();
        if !ids.is_empty() {
            let title = if selected.unit.is_empty() {
                group_name.to_string()
            } else {
                format!("{group_name} ({})", selected.unit)
            };
            return Widget::chart(Query::series(SeriesSelector::ids(ids), TimeRange::All))
                .with_title(title);
        }
    }

    Widget::chart(Query::series(
        SeriesSelector::ids(vec![selected.id.clone()]),
        TimeRange::All,
    ))
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

fn rows_from_text(text: &str) -> Vec<Vec<String>> {
    text.lines().map(|line| vec![line.to_string()]).collect()
}

pub fn rows_from_query(result: &QueryResult) -> Vec<Vec<String>> {
    result
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .enumerate()
                .map(|(index, cell)| {
                    let unit = match &row.series {
                        Some(series) if result.value_column == Some(index) => series.unit.as_str(),
                        _ => "",
                    };
                    format_cell(cell, unit)
                })
                .collect()
        })
        .collect()
}

fn format_cell(value: &SeriesValue, unit: &str) -> String {
    match value {
        SeriesValue::Number(value) => {
            if unit.is_empty() {
                format_number(*value)
            } else {
                format!("{} {}", format_number(*value), unit)
            }
        }
        SeriesValue::Text(value) => value.clone(),
    }
}

pub(crate) fn format_number(value: f64) -> String {
    let fixed = format!("{value:.2}");
    fixed
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

pub enum PanelMove {
    Left,
    Right,
    Up,
    Down,
}

pub fn panel_navigate(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut AppState,
    direction: PanelMove,
) -> std::io::Result<bool> {
    let size = terminal.size()?;
    let is_wide = size.width >= 90;

    if is_wide {
        match direction {
            PanelMove::Left => match app.focus() {
                FocusPanel::Chart => app.set_focus(FocusPanel::Metrics),
                FocusPanel::Info => app.set_focus(FocusPanel::Collectors),
                _ => {}
            },
            PanelMove::Right => match app.focus() {
                FocusPanel::Metrics => app.set_focus(FocusPanel::Chart),
                FocusPanel::Collectors => app.set_focus(FocusPanel::Info),
                _ => {}
            },
            PanelMove::Up => match app.focus() {
                FocusPanel::Collectors => app.set_focus(FocusPanel::Metrics),
                FocusPanel::Info => app.set_focus(FocusPanel::Chart),
                _ => {}
            },
            PanelMove::Down => match app.focus() {
                FocusPanel::Metrics => app.set_focus(FocusPanel::Collectors),
                FocusPanel::Chart => app.set_focus(FocusPanel::Info),
                _ => {}
            },
        }
        return Ok(false);
    }

    match direction {
        PanelMove::Up | PanelMove::Down => {
            let order = [
                FocusPanel::Metrics,
                FocusPanel::Chart,
                FocusPanel::Info,
                FocusPanel::Collectors,
            ];
            let current = order
                .iter()
                .position(|item| *item == app.focus())
                .unwrap_or(0);
            let next = if matches!(direction, PanelMove::Up) {
                current.saturating_sub(1)
            } else {
                (current + 1).min(order.len() - 1)
            };
            app.set_focus(order[next]);
        }
        PanelMove::Left | PanelMove::Right => {}
    }

    Ok(false)
}
