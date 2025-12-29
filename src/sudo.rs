use std::io;
use std::process::Command;

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::tui::AppState;

pub fn prompt_for_sudo(message: &str) -> io::Result<bool> {
    if !message.is_empty() {
        println!("{message}");
    }

    let status = Command::new("sudo").arg("-v").status()?;
    Ok(status.success())
}

pub fn reauthenticate(
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

fn suspend_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn resume_tui(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> io::Result<()> {
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}
