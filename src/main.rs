mod collector;
mod data;
mod input;
mod query;
mod tui;

use std::collections::HashMap;
use std::io;
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver, Sender},
    Arc,
};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::collector::{
    needs_sudo, spawn_collector, CollectorMessage, CpuInfoCollector, CpuUsageCollector,
    GpuInfoCollector, MemoryInfoCollector, MemoryUsageCollector, OsVersionCollector,
    PowermetricsCollector, SwapUsageCollector, UptimeCollector,
};
use crate::data::{CollectorRegistration, DashboardStore, DataStore, SeriesValue};
use crate::input::InputBindings;
use crate::query::{Query, QueryProvider, QueryResult, QueryRow, QuerySource, SeriesMode};
use crate::tui::{draw_ui, panel_at, rows_from_query, AppState, FocusPanel, NoticeAction};

const COLLECTOR_ACTIONS: [&str; 2] = ["on", "off"];

fn main() -> io::Result<()> {
    if !cfg!(target_os = "macos") {
        eprintln!("mbp-info only supports macOS collectors.");
        return Ok(());
    }

    let use_sudo = needs_sudo();
    let registrations = register_collectors(use_sudo);
    let mut sudo_ready = false;
    let mut startup_message = None;

    if use_sudo && registrations.iter().any(|reg| reg.requires_sudo) {
        sudo_ready = prompt_for_sudo("Some collectors need sudo. Authenticate now?")?;
        if !sudo_ready {
            startup_message = Some(
                "Sudo collectors disabled. Use :reauth <collector> then :collector on <collector>."
                    .to_string(),
            );
        }
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = mpsc::channel();
    let mut manager = CollectorManager::new(registrations, tx);
    if let Some(message) = start_default_collectors(&mut manager, use_sudo, sudo_ready) {
        if startup_message.is_none() {
            startup_message = Some(message);
        }
    }

    let result = run_app(
        &mut terminal,
        rx,
        manager,
        use_sudo,
        sudo_ready,
        startup_message,
    );

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(error) = result {
        eprintln!("{error}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    rx: Receiver<CollectorMessage>,
    mut manager: CollectorManager,
    use_sudo: bool,
    mut sudo_ready: bool,
    startup_message: Option<String>,
) -> io::Result<()> {
    let mut data = DataStore::new();
    let mut dashboards = DashboardStore::new();
    let mut app = AppState::new();
    let mut input = InputBindings::new();
    if let Some(message) = startup_message {
        app.set_message(Some(message));
    }

    let tick_rate = Duration::from_millis(200);

    loop {
        while let Ok(message) = rx.try_recv() {
            match message {
                CollectorMessage::Sample {
                    collector,
                    data: sample,
                } => {
                    manager.record_sample(collector);
                    data.apply_sample(sample);
                    app.clamp_selection(&data);
                }
                CollectorMessage::Error { collector, error } => {
                    app.set_error(Some(format!("{collector}: {error}")));
                }
            }
        }

        app.set_collectors_count(manager.collector_count());
        terminal.draw(|frame| draw_ui(frame, &data, &manager, &dashboards, &app))?;

        if event::poll(tick_rate)? {
            match event::read()? {
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }

                    if app.is_command_mode() {
                        if handle_command_input(
                            key.code,
                            &mut app,
                            &mut input,
                            &data,
                            &mut dashboards,
                            terminal,
                            &mut manager,
                            use_sudo,
                            &mut sudo_ready,
                        )? {
                            return Ok(());
                        }
                        continue;
                    }

                    if app.has_notice() {
                        match key.code {
                            KeyCode::Esc => {
                                app.clear_notice();
                                input.clear_pending();
                                continue;
                            }
                            KeyCode::Char('q') if key.modifiers.is_empty() => {
                                app.clear_notice();
                                input.clear_pending();
                                continue;
                            }
                            KeyCode::Enter => {
                                if handle_notice_enter(&mut app, &mut dashboards) {
                                    input.clear_pending();
                                    continue;
                                }
                            }
                            _ => {}
                        }
                    }

                    match key.code {
                        KeyCode::Char(':') => {
                            app.enter_command_mode();
                            input.clear_pending();
                        }
                        _ => {
                            if let Some(command) = input.resolve(key) {
                                if execute_named_command(
                                    &command,
                                    &mut app,
                                    &data,
                                    &mut dashboards,
                                    terminal,
                                    &mut manager,
                                    &input,
                                )? {
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
                Event::Mouse(mouse) => {
                    if app.is_command_mode() {
                        continue;
                    }

                    match mouse.kind {
                        MouseEventKind::ScrollUp => {
                            execute_named_command(
                                "cursor_up",
                                &mut app,
                                &data,
                                &mut dashboards,
                                terminal,
                                &mut manager,
                                &input,
                            )?;
                        }
                        MouseEventKind::ScrollDown => {
                            execute_named_command(
                                "cursor_down",
                                &mut app,
                                &data,
                                &mut dashboards,
                                terminal,
                                &mut manager,
                                &input,
                            )?;
                        }
                        MouseEventKind::Down(_) => {
                            if app.has_notice() {
                                continue;
                            }
                            let size = terminal.size()?;
                            if let Some(panel) =
                                panel_at(size, mouse.column, mouse.row, dashboards.active_type())
                            {
                                app.set_focus(panel);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
}

fn start_default_collectors(
    manager: &mut CollectorManager,
    use_sudo: bool,
    sudo_ready: bool,
) -> Option<String> {
    let mut message = None;

    let registrations: Vec<(&'static str, bool)> = manager
        .registrations()
        .iter()
        .map(|registration| (registration.id, registration.requires_sudo))
        .collect();

    for (id, requires_sudo) in registrations {
        if requires_sudo && use_sudo && !sudo_ready {
            continue;
        }
        if let Err(error) = manager.enable(id) {
            message = Some(format!("Failed to start {id}: {error}"));
        }
    }

    message
}

fn register_collectors(use_sudo: bool) -> Vec<CollectorRegistration> {
    vec![
        CollectorRegistration::new(Box::new(move || {
            Box::new(PowermetricsCollector::new(use_sudo))
        })),
        CollectorRegistration::new(Box::new(|| Box::new(CpuUsageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(MemoryUsageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(SwapUsageCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(UptimeCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(CpuInfoCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(MemoryInfoCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(GpuInfoCollector::new()))),
        CollectorRegistration::new(Box::new(|| Box::new(OsVersionCollector::new()))),
    ]
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
    execute!(terminal.backend_mut(), DisableMouseCapture, LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    execute!(terminal.backend_mut(), EnterAlternateScreen, EnableMouseCapture)?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}

fn reauthenticate(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
) -> io::Result<bool> {
    suspend_tui(terminal)?;
    let result = prompt_for_sudo("Re-authenticate sudo for collectors:");
    resume_tui(terminal)?;

    match result {
        Ok(true) => {
            app.set_message(Some("sudo authenticated".to_string()));
            Ok(true)
        }
        Ok(false) => {
            app.set_error(Some("sudo failed or was cancelled.".to_string()));
            Ok(false)
        }
        Err(error) => {
            app.set_error(Some(format!("sudo error: {error}")));
            Ok(false)
        }
    }
}

fn handle_command_input(
    code: KeyCode,
    app: &mut AppState,
    input: &mut InputBindings,
    data: &DataStore,
    dashboards: &mut DashboardStore,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    manager: &mut CollectorManager,
    use_sudo: bool,
    sudo_ready: &mut bool,
) -> io::Result<bool> {
    match code {
        KeyCode::Esc => {
            app.exit_command_mode();
        }
        KeyCode::Enter => {
            let command = app.take_command();
            app.exit_command_mode();
            app.record_command(&command);
            return run_command(
                command.trim(),
                app,
                input,
                data,
                dashboards,
                terminal,
                manager,
                use_sudo,
                sudo_ready,
            );
        }
        KeyCode::Up => {
            app.history_prev();
        }
        KeyCode::Down => {
            app.history_next();
        }
        KeyCode::Tab => {
            let collector_ids = manager.collector_ids();
            complete_command_buffer(app, &collector_ids);
        }
        KeyCode::BackTab => {
            if app.has_completion() {
                if let Some(buffer) = app.cycle_completion_prev() {
                    app.set_command_buffer_from_completion(buffer);
                }
            } else {
                let collector_ids = manager.collector_ids();
                complete_command_buffer(app, &collector_ids);
            }
        }
        KeyCode::Backspace => {
            app.pop_command_char();
        }
        KeyCode::Char(ch) => {
            app.push_command_char(ch);
        }
        _ => {}
    }

    Ok(false)
}

fn run_command(
    command: &str,
    app: &mut AppState,
    input: &mut InputBindings,
    data: &DataStore,
    dashboards: &mut DashboardStore,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    manager: &mut CollectorManager,
    use_sudo: bool,
    sudo_ready: &mut bool,
) -> io::Result<bool> {
    let mut parts = command.split_whitespace();
    let Some(root) = parts.next() else {
        return Ok(false);
    };

    match root {
        "collector" => {
            let action = parts.next().unwrap_or_default();
            let id = parts.next().unwrap_or_default();
            if action.is_empty() || id.is_empty() {
                app.set_error(Some("Usage: :collector on|off <id>".to_string()));
                return Ok(false);
            }

            match action {
                "on" | "enable" | "start" => {
                    if manager.is_enabled(id) {
                        app.set_message(Some(format!("Collector '{id}' already enabled.")));
                        return Ok(false);
                    }
                    if manager.requires_sudo(id).unwrap_or(false) && use_sudo && !*sudo_ready {
                        if reauthenticate(terminal, app)? {
                            *sudo_ready = true;
                        } else {
                            return Ok(false);
                        }
                    }
                    match manager.enable(id) {
                        Ok(()) => app.set_message(Some(format!("Collector '{id}' enabled."))),
                        Err(error) => app.set_error(Some(error)),
                    }
                    Ok(false)
                }
                "off" | "disable" | "stop" => {
                    match manager.disable(id) {
                        Ok(()) => app.set_message(Some(format!("Collector '{id}' disabled."))),
                        Err(error) => app.set_error(Some(error)),
                    }
                    Ok(false)
                }
                _ => {
                    app.set_error(Some("Usage: :collector on|off <id>".to_string()));
                    Ok(false)
                }
            }
        }
        "reauth" => {
            let collector = parts.next().unwrap_or_default();
            if collector.is_empty() {
                app.set_error(Some("Usage: :reauth <collector>".to_string()));
                return Ok(false);
            }
            match manager.requires_sudo(collector) {
                Some(true) => {
                    if use_sudo {
                        let ok = reauthenticate(terminal, app)?;
                        *sudo_ready = ok;
                    } else {
                        app.set_error(Some("Already running without sudo.".to_string()));
                    }
                }
                Some(false) => {
                    app.set_message(Some(format!(
                        "Collector '{collector}' does not require sudo."
                    )));
                }
                None => {
                    app.set_error(Some(format!("Unknown collector: {collector}")));
                }
            }
            Ok(false)
        }
        "map" => {
            let sequence = parts.next().unwrap_or_default();
            if sequence.is_empty() {
                app.set_notice("Mappings", input.mapping_rows(), false);
                return Ok(false);
            }
            let command = parts.next().unwrap_or_default();
            if command.is_empty() {
                app.set_error(Some("Usage: :map <keys> <command>".to_string()));
                return Ok(false);
            }
            if !InputBindings::is_command_known(command) {
                app.set_error(Some(format!("Unknown command: {command}")));
                return Ok(false);
            }
            match input.bind(sequence, command) {
                Ok(()) => app.set_message(Some(format!("Mapped {sequence} -> {command}"))),
                Err(error) => app.set_error(Some(error)),
            }
            Ok(false)
        }
        "unmap" => {
            let sequence = parts.next().unwrap_or_default();
            if sequence.is_empty() {
                app.set_error(Some("Usage: :unmap <keys>".to_string()));
                return Ok(false);
            }
            match input.unbind(sequence) {
                Ok(()) => app.set_message(Some(format!("Unmapped {sequence}"))),
                Err(error) => app.set_error(Some(error)),
            }
            Ok(false)
        }
        "dashboard_delete" => {
            let index = if let Some(value) = parts.next() {
                match parse_dashboard_index(value) {
                    Ok(index) => index,
                    Err(error) => {
                        app.set_error(Some(error));
                        return Ok(false);
                    }
                }
            } else {
                dashboards.active_index()
            };
            match dashboards.remove(index) {
                Ok(()) => app.set_message(Some(format!("Removed dashboard {}.", index + 1))),
                Err(error) => app.set_error(Some(error)),
            }
            Ok(false)
        }
        "dashboard_goto" => {
            let Some(value) = parts.next() else {
                app.set_error(Some("Usage: :dashboard_goto <n>".to_string()));
                return Ok(false);
            };
            let index = match parse_dashboard_index(value) {
                Ok(index) => index,
                Err(error) => {
                    app.set_error(Some(error));
                    return Ok(false);
                }
            };
            match dashboards.set_active(index) {
                Ok(()) => {}
                Err(error) => app.set_error(Some(error)),
            }
            Ok(false)
        }
        "maps" => {
            app.set_notice("Mappings", input.mapping_rows(), false);
            Ok(false)
        }
        _ if InputBindings::is_no_arg_command(root) => {
            if parts.next().is_some() {
                app.set_error(Some(format!(":{root} does not take arguments.")));
                return Ok(false);
            }
            execute_named_command(root, app, data, dashboards, terminal, manager, input)
        }
        _ => {
            app.set_error(Some(format!("Unknown command: {root}")));
            Ok(false)
        }
    }
}

fn execute_named_command(
    command: &str,
    app: &mut AppState,
    data: &DataStore,
    dashboards: &mut DashboardStore,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    manager: &mut CollectorManager,
    input: &InputBindings,
) -> io::Result<bool> {
    if app.has_notice() {
        match command {
            "cursor_up" => {
                app.select_notice_prev();
                return Ok(false);
            }
            "cursor_down" => {
                app.select_notice_next();
                return Ok(false);
            }
            "jump_to_first_row" => {
                app.jump_notice_top();
                return Ok(false);
            }
            "jump_to_last_row" => {
                app.jump_notice_bottom();
                return Ok(false);
            }
            _ => {}
        }
    }

    match command {
        "q" | "quit" => Ok(true),
        "help" => {
            app.set_notice("Help", build_help_rows(), false);
            Ok(false)
        }
        "collectors" => {
            let query = Query {
                source: QuerySource::Collectors,
                selected_index: Some(app.collectors_selected()),
                mode: SeriesMode::Latest,
            };
            let result = manager.query(query);
            let rows = rows_from_query(&result);
            app.set_notice(&result.title, rows, false);
            Ok(false)
        }
        "dashboards" => {
            let query = Query {
                source: QuerySource::Dashboards,
                selected_index: None,
                mode: SeriesMode::Latest,
            };
            let result = dashboards.query(query);
            let rows = rows_from_query(&result);
            app.set_notice_with_action(&result.title, rows, false, NoticeAction::DashboardGoto);
            Ok(false)
        }
        "dashboard_new" => {
            dashboards.add_blank();
            Ok(false)
        }
        "dashboard_next" => {
            dashboards.next();
            Ok(false)
        }
        "dashboard_prev" => {
            dashboards.prev();
            Ok(false)
        }
        "maps" => {
            app.set_notice("Mappings", input.mapping_rows(), false);
            Ok(false)
        }
        "jump_to_first_row" => {
            app.jump_top(data);
            Ok(false)
        }
        "jump_to_last_row" => {
            app.jump_bottom(data);
            Ok(false)
        }
        "cursor_up" => {
            match app.focus() {
                FocusPanel::Metrics => app.select_prev(data),
                FocusPanel::Info => app.select_info_prev(data),
                FocusPanel::Collectors => app.select_collectors_prev(),
                FocusPanel::Chart => {}
            }
            Ok(false)
        }
        "cursor_down" => {
            match app.focus() {
                FocusPanel::Metrics => app.select_next(data),
                FocusPanel::Info => app.select_info_next(data),
                FocusPanel::Collectors => app.select_collectors_next(),
                FocusPanel::Chart => {}
            }
            Ok(false)
        }
        "panel_navigate_left" => panel_navigate(terminal, app, PanelMove::Left),
        "panel_navigate_right" => panel_navigate(terminal, app, PanelMove::Right),
        "panel_navigate_up" => panel_navigate(terminal, app, PanelMove::Up),
        "panel_navigate_down" => panel_navigate(terminal, app, PanelMove::Down),
        _ => {
            app.set_error(Some(format!("Unknown command: {command}")));
            Ok(false)
        }
    }
}

fn build_help_rows() -> Vec<Vec<String>> {
    vec![
        vec![":q".to_string(), "quit".to_string()],
        vec![":help".to_string(), "show this help".to_string()],
        vec![":collectors".to_string(), "list collectors".to_string()],
        vec![":dashboards".to_string(), "list dashboards".to_string()],
        vec![":dashboard_new".to_string(), "create dashboard".to_string()],
        vec![
            ":dashboard_delete [n]".to_string(),
            "remove dashboard".to_string(),
        ],
        vec![":dashboard_next".to_string(), "next dashboard".to_string()],
        vec![":dashboard_prev".to_string(), "previous dashboard".to_string()],
        vec![
            ":dashboard_goto <n>".to_string(),
            "switch dashboard".to_string(),
        ],
        vec![
            ":collector on <id>".to_string(),
            "enable collector".to_string(),
        ],
        vec![
            ":collector off <id>".to_string(),
            "disable collector".to_string(),
        ],
        vec![
            ":reauth <collector>".to_string(),
            "sudo re-auth for collector".to_string(),
        ],
        vec![":map <keys> <command>".to_string(), "bind keys".to_string()],
        vec![":unmap <keys>".to_string(), "remove key bind".to_string()],
        vec![":maps".to_string(), "list key binds".to_string()],
        vec![":jump_to_first_row".to_string(), "jump to top".to_string()],
        vec![
            ":jump_to_last_row".to_string(),
            "jump to bottom".to_string(),
        ],
        vec![":cursor_up".to_string(), "move selection up".to_string()],
        vec![
            ":cursor_down".to_string(),
            "move selection down".to_string(),
        ],
        vec![
            ":panel_navigate_left/right/up/down".to_string(),
            "move focus between panels".to_string(),
        ],
    ]
}

enum PanelMove {
    Left,
    Right,
    Up,
    Down,
}

fn panel_navigate(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
    direction: PanelMove,
) -> io::Result<bool> {
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

fn complete_command_buffer(app: &mut AppState, collectors: &[&'static str]) {
    if app.has_completion() {
        if let Some(buffer) = app.cycle_completion() {
            app.set_command_buffer_from_completion(buffer);
        }
        return;
    }

    let buffer = app.command_buffer().to_string();
    let trimmed = buffer.trim_start();
    if trimmed.is_empty() {
        return;
    }

    let trailing_space = buffer.ends_with(' ');
    let tokens: Vec<&str> = buffer.split_whitespace().collect();
    if tokens.is_empty() {
        return;
    }

    if tokens.len() == 1 && !trailing_space {
        let prefix = tokens[0];
        let matches = collect_matches(crate::input::COMMANDS, prefix, |_| true);
        apply_completion(app, &buffer, prefix, matches);
        return;
    }

    if tokens[0] == "reauth" {
        complete_collector_argument(app, &buffer, tokens.as_slice(), trailing_space, collectors);
        return;
    }

    if tokens[0] == "collector" {
        if tokens.len() == 1 && trailing_space {
            apply_completion(
                app,
                &buffer,
                "",
                COLLECTOR_ACTIONS.iter().map(|value| value.to_string()).collect(),
            );
            return;
        }

        if tokens.len() == 2 && !trailing_space {
            let prefix = tokens[1];
            let matches = collect_matches(&COLLECTOR_ACTIONS, prefix, |_| true);
            apply_completion(app, &buffer, prefix, matches);
            return;
        }

        if tokens.len() >= 2 {
            complete_collector_argument(
                app,
                &buffer,
                tokens.as_slice(),
                trailing_space,
                collectors,
            );
        }
        return;
    }

    if tokens[0] == "map" {
        if tokens.len() == 2 && trailing_space {
            let matches = collect_matches(crate::input::COMMANDS, "", |command| {
                InputBindings::is_no_arg_command(command)
            });
            apply_completion(app, &buffer, "", matches);
            return;
        }

        if tokens.len() == 3 && !trailing_space {
            let prefix = tokens[2];
            let matches = collect_matches(crate::input::COMMANDS, prefix, |command| {
                InputBindings::is_no_arg_command(command)
            });
            apply_completion(app, &buffer, prefix, matches);
        }
    }
}

fn complete_collector_argument(
    app: &mut AppState,
    buffer: &str,
    tokens: &[&str],
    trailing_space: bool,
    collectors: &[&'static str],
) {
    if tokens.len() == 1 && trailing_space {
        apply_completion(
            app,
            buffer,
            "",
            collectors.iter().copied().map(|value| value.to_string()).collect(),
        );
        return;
    }

    if trailing_space && tokens.len() >= 2 {
        apply_completion(
            app,
            buffer,
            "",
            collectors.iter().copied().map(|value| value.to_string()).collect(),
        );
        return;
    }

    if tokens.len() >= 2 && !trailing_space {
        let prefix = tokens[tokens.len() - 1];
        let matches: Vec<String> = collectors
            .iter()
            .copied()
            .filter(|collector| collector.starts_with(prefix))
            .map(|collector| collector.to_string())
            .collect();
        apply_completion(app, buffer, prefix, matches);
    }
}

fn apply_completion(app: &mut AppState, buffer: &str, prefix: &str, matches: Vec<String>) {
    if matches.is_empty() {
        return;
    }

    let mut candidates = matches;
    candidates.sort();

    if candidates.len() == 1 {
        let base = completion_base(buffer, prefix);
        app.set_command_buffer(format!("{base}{}", candidates[0]));
        return;
    }

    let base = completion_base(buffer, prefix);
    app.set_completion(base.clone(), candidates.clone());
    app.set_command_buffer_from_completion(format!("{base}{}", candidates[0]));
}

fn completion_base(buffer: &str, prefix: &str) -> String {
    let cut = buffer.len().saturating_sub(prefix.len());
    buffer[..cut].to_string()
}

fn collect_matches<F>(entries: &[&str], prefix: &str, predicate: F) -> Vec<String>
where
    F: Fn(&str) -> bool,
{
    entries
        .iter()
        .copied()
        .filter(|entry| entry.starts_with(prefix) && predicate(entry))
        .map(|entry| entry.to_string())
        .collect()
}

fn handle_notice_enter(app: &mut AppState, dashboards: &mut DashboardStore) -> bool {
    match app.notice_action() {
        Some(NoticeAction::DashboardGoto) => {
            let index = app.notice_selected();
            match dashboards.set_active(index) {
                Ok(()) => app.clear_notice(),
                Err(error) => app.set_error(Some(error)),
            }
            true
        }
        None => false,
    }
}

fn parse_dashboard_index(value: &str) -> Result<usize, String> {
    let parsed: usize = value
        .parse()
        .map_err(|_| format!("Invalid dashboard index: {value}"))?;
    if parsed == 0 {
        return Err("Dashboard index starts at 1.".to_string());
    }
    Ok(parsed - 1)
}

struct CollectorHandle {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

struct CollectorManager {
    registrations: Vec<CollectorRegistration>,
    running: HashMap<&'static str, CollectorHandle>,
    last_sample: HashMap<&'static str, Instant>,
    tx: Sender<CollectorMessage>,
}

impl CollectorManager {
    fn new(registrations: Vec<CollectorRegistration>, tx: Sender<CollectorMessage>) -> Self {
        Self {
            registrations,
            running: HashMap::new(),
            last_sample: HashMap::new(),
            tx,
        }
    }

    fn registrations(&self) -> &[CollectorRegistration] {
        &self.registrations
    }

    fn collector_count(&self) -> usize {
        self.registrations.len()
    }

    fn collector_ids(&self) -> Vec<&'static str> {
        self.registrations.iter().map(|reg| reg.id).collect()
    }

    fn collector_rows(&self) -> Vec<QueryRow> {
        let now = Instant::now();
        self.registrations
            .iter()
            .map(|reg| {
                let status = if self.running.contains_key(reg.id) {
                    "on"
                } else {
                    "off"
                };
                let status = if reg.requires_sudo {
                    format!("{status} (sudo)")
                } else {
                    status.to_string()
                };
                let interval = format!("{}s", reg.interval.as_secs());
                let last_update = self
                    .last_sample
                    .get(reg.id)
                    .map(|instant| format!("{}s ago", now.duration_since(*instant).as_secs()))
                    .unwrap_or_else(|| "-".to_string());
                QueryRow {
                    cells: vec![
                        vec![SeriesValue::Text(reg.name.to_string())],
                        vec![SeriesValue::Text(reg.id.to_string())],
                        vec![SeriesValue::Text(status)],
                        vec![SeriesValue::Text(interval)],
                        vec![SeriesValue::Text(last_update)],
                    ],
                    series_label: None,
                    series_unit: None,
                    sample_count: None,
                }
            })
            .collect()
    }

    fn collectors_query(&self) -> QueryResult {
        QueryResult {
            title: "Collectors".to_string(),
            columns: vec![
                "Collector".to_string(),
                "ID".to_string(),
                "Status".to_string(),
                "Interval".to_string(),
                "Last update".to_string(),
            ],
            rows: self.collector_rows(),
            series_column: None,
        }
    }

    fn requires_sudo(&self, id: &str) -> Option<bool> {
        self.registrations
            .iter()
            .find(|reg| reg.id == id)
            .map(|reg| reg.requires_sudo)
    }

    fn is_enabled(&self, id: &str) -> bool {
        self.running.contains_key(id)
    }

    fn record_sample(&mut self, id: &'static str) {
        self.last_sample.insert(id, Instant::now());
    }

    fn enable(&mut self, id: &str) -> Result<(), String> {
        if self.running.contains_key(id) {
            return Err(format!("Collector '{id}' already enabled."));
        }

        let reg = self
            .registrations
            .iter()
            .find(|reg| reg.id == id)
            .ok_or_else(|| format!("Unknown collector: {id}"))?;

        let collector = (reg.factory)();
        let stop = Arc::new(AtomicBool::new(true));
        let handle = spawn_collector(collector, self.tx.clone(), stop.clone());

        self.running.insert(
            reg.id,
            CollectorHandle {
                stop,
                join: Some(handle),
            },
        );

        Ok(())
    }

    fn disable(&mut self, id: &str) -> Result<(), String> {
        let mut handle = self
            .running
            .remove(id)
            .ok_or_else(|| format!("Collector '{id}' is not enabled."))?;

        handle.stop.store(false, Ordering::Relaxed);
        if let Some(join) = handle.join.take() {
            let _ = join.join();
        }

        Ok(())
    }
}

impl QueryProvider for CollectorManager {
    fn query(&self, query: Query) -> QueryResult {
        match query.source {
            QuerySource::Collectors => self.collectors_query(),
            _ => QueryResult {
                title: "Unsupported".to_string(),
                columns: Vec::new(),
                rows: Vec::new(),
                series_column: None,
            },
        }
    }
}
