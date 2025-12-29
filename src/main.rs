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

use crate::collector::{needs_sudo, spawn_powermetrics, CollectorMessage};
use crate::data::DataStore;
use crate::tui::{draw_ui, AppState};

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
    let mut app = AppState::new(use_sudo);
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

                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    KeyCode::Char('a') if use_sudo => {
                        reauthenticate(terminal, &mut app)?;
                    }
                    KeyCode::Up => app.select_prev(&data),
                    KeyCode::Down => app.select_next(&data),
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
