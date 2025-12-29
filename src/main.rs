mod collector;
mod data;
mod tui;

use std::io;
use std::process::Command;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::collector::{needs_sudo, spawn_powermetrics, CollectorMessage, COLLECTORS};
use crate::data::DataStore;
use crate::tui::{draw_ui, AppState, FocusPanel};

const COMMANDS: [&str; 5] = ["q", "quit", "reauth", "help", "collectors"];

fn main() -> io::Result<()> {
    if !cfg!(target_os = "macos") {
        eprintln!("mbp-info only supports macOS powermetrics.");
        return Ok(());
    }

    let use_sudo = needs_sudo();
    if use_sudo {
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
    spawn_powermetrics(tx, use_sudo);

    let result = run_app(&mut terminal, rx, use_sudo);

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
    rx: Receiver<CollectorMessage>,
    use_sudo: bool,
) -> io::Result<()> {
    let mut data = DataStore::new();
    let mut app = AppState::new();
    let tick_rate = Duration::from_millis(200);

    loop {
        while let Ok(message) = rx.try_recv() {
            match message {
                CollectorMessage::Sample(sample) => {
                    data.apply_sample(sample);
                    app.clamp_selection(&data);
                    app.set_error(None);
                }
                CollectorMessage::Error(error) => app.set_error(Some(error)),
            }
        }

        terminal.draw(|frame| draw_ui(frame, &data, &app))?;

        if event::poll(tick_rate)? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                if app.is_command_mode() {
                    if handle_command_input(key.code, &mut app, terminal, use_sudo)? {
                        return Ok(());
                    }
                    continue;
                }

                if app.focus_pending() {
                    app.set_focus_pending(false);
                    if handle_focus_move(terminal, &mut app, key.code)? {
                        continue;
                    }
                }

                match key.code {
                    KeyCode::Char(':') => app.enter_command_mode(),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        app.set_focus_pending(true);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        if app.focus() == FocusPanel::Metrics {
                            app.select_prev(&data);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if app.focus() == FocusPanel::Metrics {
                            app.select_next(&data);
                        }
                    }
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
    app: &mut AppState,
) -> io::Result<()> {
    suspend_tui(terminal)?;
    let result = prompt_for_sudo("Re-authenticate sudo for powermetrics:");
    resume_tui(terminal)?;

    match result {
        Ok(true) => app.set_error(None),
        Ok(false) => app.set_error(Some("sudo failed or was cancelled.".to_string())),
        Err(error) => app.set_error(Some(format!("sudo error: {error}"))),
    }

    Ok(())
}

fn handle_command_input(
    code: KeyCode,
    app: &mut AppState,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    use_sudo: bool,
) -> io::Result<bool> {
    match code {
        KeyCode::Esc => {
            app.exit_command_mode();
        }
        KeyCode::Enter => {
            let command = app.take_command();
            app.exit_command_mode();
            app.record_command(&command);
            return run_command(&command, app, terminal, use_sudo);
        }
        KeyCode::Up => {
            app.history_prev();
        }
        KeyCode::Down => {
            app.history_next();
        }
        KeyCode::Tab => {
            complete_command_buffer(app);
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
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    use_sudo: bool,
) -> io::Result<bool> {
    let mut parts = command.split_whitespace();
    let Some(root) = parts.next() else {
        return Ok(false);
    };

    match root {
        "q" | "quit" => Ok(true),
        "help" => {
            app.set_message(Some(
                "Commands: :q, :help, :collectors, :reauth <collector>".to_string(),
            ));
            Ok(false)
        }
        "collectors" => {
            app.set_message(Some(format!(
                "Collectors: {}",
                COLLECTORS.join(", ")
            )));
            Ok(false)
        }
        "reauth" => {
            let collector = parts.next().unwrap_or_default();
            if collector.is_empty() {
                app.set_error(Some("Usage: :reauth <collector>".to_string()));
                return Ok(false);
            }
            if collector == "powermetrics" {
                if use_sudo {
                    reauthenticate(terminal, app)?;
                } else {
                    app.set_error(Some("Already running without sudo.".to_string()));
                }
            } else {
                app.set_error(Some(format!("Unknown collector: {collector}")));
            }
            Ok(false)
        }
        _ => {
            app.set_error(Some(format!("Unknown command: {root}")));
            Ok(false)
        }
    }
}

fn complete_command_buffer(app: &mut AppState) {
    let buffer = app.command_buffer();
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
        let matches: Vec<&str> = COMMANDS
            .iter()
            .copied()
            .filter(|command| command.starts_with(prefix))
            .collect();
        if let Some(completion) = resolve_completion(prefix, &matches) {
            let new_buffer = replace_tail(buffer, prefix, &completion);
            app.set_command_buffer(new_buffer);
        }
        return;
    }

    if tokens[0] == "reauth" {
        if tokens.len() == 1 && trailing_space {
            let matches: Vec<&str> = COLLECTORS.iter().copied().collect();
            if let Some(completion) = resolve_completion("", &matches) {
                let mut new_buffer = buffer.to_string();
                new_buffer.push_str(&completion);
                app.set_command_buffer(new_buffer);
            }
            return;
        }

        if tokens.len() >= 2 && !trailing_space {
            let prefix = tokens[tokens.len() - 1];
            let matches: Vec<&str> = COLLECTORS
                .iter()
                .copied()
                .filter(|collector| collector.starts_with(prefix))
                .collect();
            if let Some(completion) = resolve_completion(prefix, &matches) {
                let new_buffer = replace_tail(buffer, prefix, &completion);
                app.set_command_buffer(new_buffer);
            }
        }
    }
}

fn resolve_completion(prefix: &str, matches: &[&str]) -> Option<String> {
    if matches.is_empty() {
        return None;
    }

    let completion = if matches.len() == 1 {
        matches[0].to_string()
    } else {
        common_prefix(matches)
    };

    if completion.len() > prefix.len() {
        Some(completion)
    } else {
        None
    }
}

fn common_prefix(matches: &[&str]) -> String {
    let first = matches[0].as_bytes();
    let mut length = first.len();
    for entry in matches.iter().skip(1) {
        let bytes = entry.as_bytes();
        length = length.min(bytes.len());
        for i in 0..length {
            if bytes[i] != first[i] {
                length = i;
                break;
            }
        }
    }
    String::from_utf8_lossy(&first[..length]).to_string()
}

fn replace_tail(buffer: &str, prefix: &str, completion: &str) -> String {
    let cut = buffer.len().saturating_sub(prefix.len());
    let mut new_buffer = buffer.to_string();
    new_buffer.replace_range(cut.., completion);
    new_buffer
}

fn handle_focus_move(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut AppState,
    code: KeyCode,
) -> io::Result<bool> {
    let size = terminal.size()?;
    let is_wide = size.width >= 90;

    match code {
        KeyCode::Left | KeyCode::Char('h') => {
            if is_wide {
                app.set_focus(FocusPanel::Metrics);
                return Ok(true);
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if is_wide {
                app.set_focus(FocusPanel::Chart);
                return Ok(true);
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            move_focus_vertical(app, is_wide, true);
            return Ok(true);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            move_focus_vertical(app, is_wide, false);
            return Ok(true);
        }
        KeyCode::Esc => {
            return Ok(true);
        }
        _ => {}
    }

    Ok(false)
}

fn move_focus_vertical(app: &mut AppState, is_wide: bool, up: bool) {
    if is_wide {
        match (app.focus(), up) {
            (FocusPanel::Info, true) => app.set_focus(FocusPanel::Chart),
            (FocusPanel::Chart, false) => app.set_focus(FocusPanel::Info),
            _ => {}
        }
    } else {
        let order = [FocusPanel::Metrics, FocusPanel::Chart, FocusPanel::Info];
        let current = order
            .iter()
            .position(|item| *item == app.focus())
            .unwrap_or(0);
        let next = if up {
            current.saturating_sub(1)
        } else {
            (current + 1).min(order.len() - 1)
        };
        app.set_focus(order[next]);
    }
}
