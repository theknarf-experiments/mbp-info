use std::time::SystemTime;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Frame,
    style::{Color, Style},
    symbols,
    text::Span,
    widgets::{Axis, Block, Borders, Chart, Dataset, Paragraph},
};

use crate::data::SeriesValue;
use crate::query::{
    Query, QueryProvider, QueryResult, QuerySource, SeriesKind, SeriesSelector, TimeRange,
};
use crate::tui::{draw_tui_table, format_number, rows_from_query, TuiTable};

const CHART_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::LightBlue,
    Color::LightRed,
];

#[derive(Clone, Debug, Default)]
pub struct TableOptions {
    /// Add a "Samples" column, filled in for the selected row only.
    pub show_samples: bool,
    pub right_align_last: bool,
}

#[derive(Clone, Debug)]
pub enum WidgetKind {
    Table(TableOptions),
    Chart,
}

/// A dashboard building block: a query plus how to present its result.
#[derive(Clone, Debug)]
pub struct Widget {
    pub kind: WidgetKind,
    pub query: Query,
    pub title: Option<String>,
    pub empty_text: Option<String>,
}

impl Widget {
    pub fn table(query: Query, options: TableOptions) -> Self {
        Self {
            kind: WidgetKind::Table(options),
            query,
            title: None,
            empty_text: None,
        }
    }

    pub fn chart(query: Query) -> Self {
        Self {
            kind: WidgetKind::Chart,
            query,
            title: None,
            empty_text: None,
        }
    }

    /// Parses `<table|chart> [range] <pattern>`. The pattern is the rest of
    /// the input, so it may contain spaces.
    pub fn parse(args: &str) -> Result<Self, String> {
        let usage = || {
            format!(
                "Usage: :widget_add <{}> [range] <pattern>",
                WIDGET_KINDS.join("|")
            )
        };
        let (kind, rest) = split_word(args).ok_or_else(usage)?;
        let (range, pattern) = match split_word(rest) {
            Some((word, after)) => match TimeRange::parse(word) {
                Some(range) => (Some(range), after),
                None => (None, rest),
            },
            None => (None, rest),
        };
        let pattern = pattern.trim();
        if pattern.is_empty() {
            return Err(usage());
        }

        let selector = SeriesSelector::pattern(pattern);
        let widget = match kind {
            "table" => Widget::table(
                Query::series(selector, range.unwrap_or(TimeRange::Latest)),
                TableOptions::default(),
            ),
            "chart" => Widget::chart(Query::series(selector, range.unwrap_or(TimeRange::All))),
            _ => return Err(format!("Unknown widget kind: {kind}. {}", usage())),
        };
        Ok(widget
            .with_title(pattern)
            .with_empty_text(format!("No series match {pattern}")))
    }

    /// Kind, range and source, for listing widgets.
    pub fn describe(&self) -> Vec<String> {
        let kind = match self.kind {
            WidgetKind::Table(_) => "table",
            WidgetKind::Chart => "chart",
        };
        let source = match &self.query.source {
            QuerySource::Series(selector) => describe_selector(selector),
            QuerySource::Collectors => "collectors".to_string(),
            QuerySource::Dashboards => "dashboards".to_string(),
        };
        vec![kind.to_string(), self.query.range.describe(), source]
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_empty_text(mut self, text: impl Into<String>) -> Self {
        self.empty_text = Some(text.into());
        self
    }
}

pub const WIDGET_KINDS: [&str; 2] = ["table", "chart"];

fn split_word(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    if input.is_empty() {
        return None;
    }
    Some(input.split_once(char::is_whitespace).unwrap_or((input, "")))
}

fn describe_selector(selector: &SeriesSelector) -> String {
    let mut parts = Vec::new();
    if let Some(pattern) = &selector.pattern {
        parts.push(pattern.clone());
    }
    if let Some(ids) = &selector.ids {
        parts.push(ids.join(", "));
    }
    if let Some(collector) = &selector.collector {
        parts.push(format!("collector={collector}"));
    }
    match selector.kind {
        SeriesKind::Any => {}
        SeriesKind::Metric => parts.push("metrics".to_string()),
        SeriesKind::Info => parts.push("info".to_string()),
    }
    if parts.is_empty() {
        "all series".to_string()
    } else {
        parts.join(" ")
    }
}

/// Per-frame screen state for a widget. Kept apart from the widget so the
/// widget itself stays a pure description that can be stored or saved.
#[derive(Copy, Clone, Debug, Default)]
pub struct WidgetView {
    pub focused: bool,
    pub selected: usize,
}

pub fn draw_widget(
    frame: &mut Frame,
    widget: &Widget,
    provider: &dyn QueryProvider,
    view: WidgetView,
    area: Rect,
) {
    let result = provider.query(&widget.query);
    draw_widget_result(frame, widget, &result, view, area);
}

/// Draws a widget from a result the caller already has, to avoid running the
/// same query twice.
pub fn draw_widget_result(
    frame: &mut Frame,
    widget: &Widget,
    result: &QueryResult,
    view: WidgetView,
    area: Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    match &widget.kind {
        WidgetKind::Table(options) => draw_table(frame, widget, options, result, view, area),
        WidgetKind::Chart => draw_chart(frame, widget, result, view, area),
    }
}

/// Lays widgets out in a roughly square grid.
pub fn draw_widget_grid(
    frame: &mut Frame,
    widgets: &[Widget],
    provider: &dyn QueryProvider,
    area: Rect,
) {
    if widgets.is_empty() {
        let block = Block::default().title("Dashboard").borders(Borders::ALL);
        frame.render_widget(Paragraph::new("No widgets yet.").block(block), area);
        return;
    }

    let columns = (widgets.len() as f64).sqrt().ceil() as usize;
    let rows: Vec<&[Widget]> = widgets.chunks(columns).collect();
    let row_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(even_split(rows.len()))
        .split(area);

    for (row, row_area) in rows.iter().zip(row_areas.iter()) {
        let cell_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(even_split(row.len()))
            .split(*row_area);
        for (widget, cell_area) in row.iter().zip(cell_areas.iter()) {
            draw_widget(frame, widget, provider, WidgetView::default(), *cell_area);
        }
    }
}

fn even_split(count: usize) -> Vec<Constraint> {
    (0..count)
        .map(|_| Constraint::Ratio(1, count as u32))
        .collect()
}

fn draw_table(
    frame: &mut Frame,
    widget: &Widget,
    options: &TableOptions,
    result: &QueryResult,
    view: WidgetView,
    area: Rect,
) {
    let mut columns = result.columns.clone();
    let mut rows = rows_from_query(result);

    if options.show_samples {
        columns.push("Samples".to_string());
        for (index, (row, query_row)) in rows.iter_mut().zip(&result.rows).enumerate() {
            let samples = match &query_row.series {
                Some(series) if index == view.selected => sample_label(series.sample_count),
                _ => String::new(),
            };
            row.push(samples);
        }
    }

    let title = widget.title.clone().unwrap_or_else(|| result.title.clone());
    let table = TuiTable::new(
        title,
        columns,
        rows,
        view.selected,
        view.focused,
        widget.empty_text.as_deref().unwrap_or("No data yet."),
        options.right_align_last,
    );
    draw_tui_table(frame, &table, area);
}

struct ChartSeries {
    label: String,
    points: Vec<(f64, f64)>,
}

fn draw_chart(
    frame: &mut Frame,
    widget: &Widget,
    result: &QueryResult,
    view: WidgetView,
    area: Rect,
) {
    let now = SystemTime::now();
    let series: Vec<ChartSeries> = result
        .rows
        .iter()
        .filter_map(|row| row.series.as_ref())
        .map(|series| ChartSeries {
            label: series.label.clone(),
            points: series
                .points
                .iter()
                .filter_map(|point| match point.value {
                    SeriesValue::Number(value) => Some((seconds_before(now, point.at), value)),
                    SeriesValue::Text(_) => None,
                })
                .collect(),
        })
        .collect();

    let title = widget
        .title
        .clone()
        .unwrap_or_else(|| match result.rows.as_slice() {
            [row] => match &row.series {
                Some(series) if !series.unit.is_empty() => {
                    format!("{} ({})", series.label, series.unit)
                }
                Some(series) => series.label.clone(),
                None => result.title.clone(),
            },
            _ => "Chart".to_string(),
        });

    let mut block = Block::default().title(title).borders(Borders::ALL);
    if view.focused {
        block = block.border_style(Style::default().fg(Color::Cyan));
    }

    if series.iter().all(|series| series.points.is_empty()) {
        let text = if result.rows.is_empty() {
            widget
                .empty_text
                .as_deref()
                .unwrap_or("Waiting for samples...")
        } else {
            "No data in range."
        };
        frame.render_widget(Paragraph::new(text).block(block), area);
        return;
    }

    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    let mut oldest = 0.0f64;
    for (x, value) in series.iter().flat_map(|series| series.points.iter()) {
        min = min.min(*value);
        max = max.max(*value);
        oldest = oldest.min(*x);
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

    let x_min = match widget.query.range {
        TimeRange::Last(duration) => -duration.as_secs_f64(),
        _ => oldest.min(-1.0),
    };

    let datasets: Vec<Dataset> = series
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

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(
            Axis::default()
                .title("time")
                .bounds([x_min, 0.0])
                .labels(vec![Span::raw(format_age(-x_min)), Span::raw("now")]),
        )
        .y_axis(
            Axis::default()
                .title("value")
                .bounds([y_min, y_max])
                .labels(vec![
                    Span::raw(format_number(y_min)),
                    Span::raw(format_number((y_min + y_max) / 2.0)),
                    Span::raw(format_number(y_max)),
                ]),
        );

    frame.render_widget(chart, area);
}

/// X position for a point: seconds before `now`, as a negative number.
fn seconds_before(now: SystemTime, at: SystemTime) -> f64 {
    -now.duration_since(at)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

fn format_age(seconds: f64) -> String {
    let seconds = seconds.round() as u64;
    if seconds < 120 {
        format!("-{seconds}s")
    } else if seconds < 7200 {
        format!("-{}m", seconds / 60)
    } else {
        format!("-{}h", seconds / 3600)
    }
}

fn sample_label(count: u64) -> String {
    if count == 1 {
        "1 sample".to_string()
    } else {
        format!("{count} samples")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn parses_widget_with_range_and_spaced_pattern() {
        let widget = Widget::parse("chart 5m cpu_usage/CPU Core*").unwrap();
        assert!(matches!(widget.kind, WidgetKind::Chart));
        assert!(matches!(
            widget.query.range,
            TimeRange::Last(duration) if duration == Duration::from_secs(300)
        ));
        assert_eq!(
            widget.describe(),
            ["chart", "last 5m", "cpu_usage/CPU Core*"]
        );
    }

    #[test]
    fn range_is_optional() {
        let widget = Widget::parse("table powermetrics/*").unwrap();
        assert_eq!(widget.describe(), ["table", "latest", "powermetrics/*"]);
        let widget = Widget::parse("chart Memory Used*").unwrap();
        assert_eq!(widget.describe(), ["chart", "all", "Memory Used*"]);
    }

    #[test]
    fn rejects_bad_input() {
        assert!(Widget::parse("").is_err());
        assert!(Widget::parse("chart").is_err());
        assert!(Widget::parse("chart 5m").is_err());
        assert!(Widget::parse("gauge *").is_err());
    }
}
