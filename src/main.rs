use std::{
    collections::{HashMap, VecDeque},
    io,
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use libc::geteuid;
use once_cell::sync::Lazy;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Frame,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Axis, Block, Borders, Chart, Dataset, List, ListItem, ListState, Paragraph, Wrap,
    },
    symbols, Terminal,
};
use regex::Regex;

const MAX_SAMPLES: usize = 120;
const SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
const CHART_COLORS: [Color; 6] = [
    Color::Cyan,
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::LightBlue,
    Color::LightRed,
];

static NUMERIC_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^\s*(-?\d+(?:\.\d+)?)(?:\s*([A-Za-z/%]+))?\s*$")
        .expect("valid regex")
});

static GROUP_RULES: &[(&str, &[&str])] = &[
    ("Die temperature", &["CPU die temperature", "GPU die temperature"]),
];

#[derive(Clone)]
struct MetricSeries {
    label: String,
    unit: String,
    values: VecDeque<f64>,
}

struct ParsedMetric {
    key: String,
    label: String,
    unit: String,
    value: f64,
}

struct ParsedInfo {
    label: String,
    value: String,
}

struct SampleData {
    metrics: Vec<ParsedMetric>,
    info: Vec<ParsedInfo>,
}

enum SampleMessage {
    Data(SampleData),
    Error(String),
}

struct App {
    metrics: HashMap<String, MetricSeries>,
    order: Vec<String>,
    selected: usize,
    sample_count: u64,
    last_sample: Option<Instant>,
    error: Option<String>,
    info: HashMap<String, String>,
    info_order: Vec<String>,
}

impl App {
    fn new() -> Self {
        Self {
            metrics: HashMap::new(),
            order: Vec::new(),
            selected: 0,
            sample_count: 0,
            last_sample: None,
            error: None,
            info: HashMap::new(),
            info_order: Vec::new(),
        }
    }

    fn apply_sample(&mut self, metrics: Vec<ParsedMetric>, info: Vec<ParsedInfo>) {
        for metric in metrics {
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

            if !self.order.contains(&metric.key) {
                self.order.push(metric.key);
            }
        }

        if !self.order.is_empty() {
            self.selected = self.selected.min(self.order.len() - 1);
        } else {
            self.selected = 0;
        }

        for entry in info {
            let is_new = !self.info.contains_key(&entry.label);
            self.info.insert(entry.label.clone(), entry.value);
            if is_new {
                self.info_order.push(entry.label);
            }
        }

        self.sample_count += 1;
        self.last_sample = Some(Instant::now());
        self.error = None;
    }

    fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    fn select_next(&mut self) {
        if self.order.is_empty() {
            return;
        }
        self.selected = (self.selected + 1).min(self.order.len() - 1);
    }

    fn select_prev(&mut self) {
        if self.order.is_empty() {
            return;
        }
        self.selected = self.selected.saturating_sub(1);
    }

    fn selected_key(&self) -> Option<&str> {
        self.order.get(self.selected).map(String::as_str)
    }
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
    app: &'a App,
    selected_key: &str,
) -> (String, Vec<&'a MetricSeries>) {
    let Some(selected) = app.metrics.get(selected_key) else {
        return ("Chart".to_string(), Vec::new());
    };

    if let Some((group_name, labels)) = group_rule_for_label(&selected.label) {
        let mut series = Vec::new();
        for label in labels {
            if let Some(found) = app.metrics.values().find(|metric| {
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

fn parse_powermetrics(text: &str) -> (Vec<ParsedMetric>, Vec<ParsedInfo>) {
    let mut metrics = Vec::new();
    let mut info = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || !trimmed.contains(':') {
            continue;
        }

        let mut parts = trimmed.splitn(2, ':');
        let label = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();
        if label.is_empty() || rest.is_empty() {
            continue;
        }

        if let Some(caps) = NUMERIC_RE.captures(rest) {
            let value: f64 = match caps.get(1).map(|m| m.as_str().parse()) {
                Some(Ok(value)) => value,
                _ => continue,
            };
            let unit = caps.get(2).map(|m| m.as_str().to_string()).unwrap_or_default();
            let key = format!("{}|{}", label, unit);

            metrics.push(ParsedMetric {
                key,
                label: label.to_string(),
                unit,
                value,
            });
        } else {
            info.push(ParsedInfo {
                label: label.to_string(),
                value: rest.to_string(),
            });
        }
    }

    (metrics, info)
}

fn sample_powermetrics() -> Result<SampleData, String> {
    let use_sudo = unsafe { geteuid() } != 0;
    let mut command = if use_sudo {
        let mut command = Command::new("sudo");
        command.arg("-n").arg("powermetrics");
        command
    } else {
        Command::new("powermetrics")
    };

    let output = command
        .args(["--samplers", "smc", "-n", "1", "-i", "1000"])
        .output()
        .map_err(|error| format!("failed to run powermetrics: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let message = if stderr.is_empty() {
            format!("powermetrics exited with status {}", output.status)
        } else if use_sudo && stderr.to_lowercase().contains("password") {
            "sudo authentication required. Press 'a' to authenticate.".to_string()
        } else {
            stderr
        };
        return Err(message);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");
    let (metrics, info) = parse_powermetrics(&combined);

    if metrics.is_empty() && info.is_empty() {
        Err("no metrics parsed from powermetrics output".to_string())
    } else {
        Ok(SampleData { metrics, info })
    }
}

fn spawn_sampler(tx: Sender<SampleMessage>) {
    thread::spawn(move || loop {
        let result = sample_powermetrics();
        let _ = match result {
            Ok(data) => tx.send(SampleMessage::Data(data)),
            Err(error) => tx.send(SampleMessage::Error(error)),
        };
        thread::sleep(SAMPLE_INTERVAL);
    });
}

fn main() -> io::Result<()> {
    if !cfg!(target_os = "macos") {
        eprintln!("mbp-info only supports macOS powermetrics.");
        return Ok(());
    }

    let is_root = unsafe { geteuid() } == 0;
    if !is_root {
        let ok = prompt_for_sudo("mbp-info needs sudo to run powermetrics. You'll be prompted now.")?;
        if !ok {
            eprintln!("sudo failed or was cancelled.");
            return Ok(());
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = mpsc::channel();
    spawn_sampler(tx);

    let result = run_app(&mut terminal, rx, !is_root);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(error) = result {
        eprintln!("{error}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    rx: Receiver<SampleMessage>,
    use_sudo: bool,
) -> io::Result<()> {
    let mut app = App::new();
    let tick_rate = Duration::from_millis(200);

    loop {
        while let Ok(message) = rx.try_recv() {
            match message {
                SampleMessage::Data(data) => app.apply_sample(data.metrics, data.info),
                SampleMessage::Error(error) => app.set_error(error),
            }
        }

        terminal.draw(|frame| draw_ui(frame, &app, use_sudo))?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Char('a') if use_sudo => {
                        reauthenticate(terminal, &mut app)?;
                    }
                    KeyCode::Up => app.select_prev(),
                    KeyCode::Down => app.select_next(),
                    _ => {}
                }
            }
        }
    }
}

fn prompt_for_sudo(message: &str) -> io::Result<bool> {
    if !message.is_empty() {
        println!("{message}");
    }

    let status = Command::new("sudo").arg("-v").status()?;
    Ok(status.success())
}

fn suspend_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

fn reauthenticate(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    suspend_tui(terminal)?;
    let result = prompt_for_sudo("Re-authenticate sudo for powermetrics:");
    resume_tui(terminal)?;

    match result {
        Ok(true) => app.error = None,
        Ok(false) => app.error = Some("sudo failed or was cancelled.".to_string()),
        Err(error) => app.error = Some(format!("sudo error: {error}")),
    }

    Ok(())
}

fn draw_ui(frame: &mut Frame, app: &App, use_sudo: bool) {
    let size = frame.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(size);

    let controls = if use_sudo {
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

        draw_metrics_list(frame, app, body_chunks[0]);
        draw_chart(frame, app, body_chunks[1]);
        draw_info_panel(frame, app, body_chunks[2]);
    } else {
        let body_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
            .split(chunks[1]);

        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(body_chunks[1]);

        draw_metrics_list(frame, app, body_chunks[0]);
        draw_chart(frame, app, right_chunks[0]);
        draw_info_panel(frame, app, right_chunks[1]);
    }

    let status = build_status_line(app);
    let footer = Paragraph::new(Line::from(status)).block(Block::default().borders(Borders::TOP));
    frame.render_widget(footer, chunks[2]);
}

fn draw_metrics_list(frame: &mut Frame, app: &App, area: Rect) {
    let entries: Vec<&MetricSeries> = app
        .order
        .iter()
        .filter_map(|key| app.metrics.get(key))
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

fn draw_chart(frame: &mut Frame, app: &App, area: Rect) {
    let Some(selected_key) = app.selected_key() else {
        let empty = Paragraph::new("Waiting for powermetrics samples...")
            .block(Block::default().title("Chart").borders(Borders::ALL));
        frame.render_widget(empty, area);
        return;
    };

    let (title, group_series) = group_series_for_selected(app, selected_key);
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

fn draw_info_panel(frame: &mut Frame, app: &App, area: Rect) {
    let max_lines = area.height.saturating_sub(2) as usize;
    let mut lines = Vec::new();

    for label in &app.info_order {
        if let Some(value) = app.info.get(label) {
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

fn build_status_line(app: &App) -> String {
    if let Some(error) = &app.error {
        return format!("Error: {error}");
    }

    if app.sample_count == 0 {
        return "Waiting for samples...".to_string();
    }

    let age = app
        .last_sample
        .map(|instant| instant.elapsed().as_secs())
        .unwrap_or(0);
    format!(
        "Samples: {} | Interval: {}s | Last update: {}s ago",
        app.sample_count,
        SAMPLE_INTERVAL.as_secs(),
        age
    )
}
