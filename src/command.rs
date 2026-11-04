use std::io;

use crossterm::event::KeyCode;
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::collector::CollectorManager;
use crate::data::{DashboardStore, DataStore};
use crate::input::InputBindings;
use crate::query::{Query, QueryProvider};
use crate::tui::{panel_navigate, rows_from_query, AppState, FocusPanel, NoticeAction, PanelMove};

const COLLECTOR_ACTIONS: [&str; 2] = ["on", "off"];

pub fn handle_command_input(
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
                        if crate::sudo::reauthenticate(terminal, app)? {
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
                        let ok = crate::sudo::reauthenticate(terminal, app)?;
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
        "leader_set" => {
            let value = parts.next().unwrap_or_default();
            if value.is_empty() {
                app.set_error(Some("Usage: :leader_set <key>".to_string()));
                return Ok(false);
            }
            let mut chars = value.chars();
            let Some(ch) = chars.next() else {
                app.set_error(Some("Usage: :leader_set <key>".to_string()));
                return Ok(false);
            };
            if chars.next().is_some() {
                app.set_error(Some("Leader key must be a single character.".to_string()));
                return Ok(false);
            }
            input.set_leader(Some(ch));
            app.set_message(Some(format!("Leader set to '{ch}'.")));
            Ok(false)
        }
        "leader_unset" => {
            input.set_leader(None);
            app.set_message(Some("Leader unset.".to_string()));
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

pub fn execute_named_command(
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
            app.set_notice_with_action(
                "Help",
                build_help_rows(),
                false,
                NoticeAction::PrefillCommand,
            );
            Ok(false)
        }
        "collectors" => {
            let result = manager.query(&Query::collectors());
            let rows = rows_from_query(&result);
            app.set_notice(&result.title, rows, false);
            Ok(false)
        }
        "dashboards" => {
            let result = dashboards.query(&Query::dashboards());
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
        vec![
            ":dashboard_prev".to_string(),
            "previous dashboard".to_string(),
        ],
        vec![
            ":dashboard_goto <n>".to_string(),
            "switch dashboard".to_string(),
        ],
        vec![
            ":leader_set <key>".to_string(),
            "set leader key".to_string(),
        ],
        vec![":leader_unset".to_string(), "unset leader key".to_string()],
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
            ":panel_navigate_left".to_string(),
            "move focus left".to_string(),
        ],
        vec![
            ":panel_navigate_right".to_string(),
            "move focus right".to_string(),
        ],
        vec![
            ":panel_navigate_up".to_string(),
            "move focus up".to_string(),
        ],
        vec![
            ":panel_navigate_down".to_string(),
            "move focus down".to_string(),
        ],
    ]
}

pub fn handle_notice_enter(app: &mut AppState, dashboards: &mut DashboardStore) -> bool {
    match app.notice_action() {
        Some(NoticeAction::DashboardGoto) => {
            let index = app.notice_selected();
            match dashboards.set_active(index) {
                Ok(()) => app.clear_notice(),
                Err(error) => app.set_error(Some(error)),
            }
            true
        }
        Some(NoticeAction::PrefillCommand) => {
            if let Some(command) = app.notice_selected_command() {
                app.clear_notice();
                app.enter_command_mode();
                app.set_command_buffer(command);
                return true;
            }
            false
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
                COLLECTOR_ACTIONS
                    .iter()
                    .map(|value| value.to_string())
                    .collect(),
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
            collectors
                .iter()
                .copied()
                .map(|value| value.to_string())
                .collect(),
        );
        return;
    }

    if trailing_space && tokens.len() >= 2 {
        apply_completion(
            app,
            buffer,
            "",
            collectors
                .iter()
                .copied()
                .map(|value| value.to_string())
                .collect(),
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
