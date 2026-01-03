mod collector;
mod command;
mod data;
mod dump;
mod input;
mod query;
mod sudo;
mod tui;

use std::env;
use std::io;
use std::sync::mpsc;
use std::time::Duration;

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

#[cfg(target_os = "macos")]
use crate::collector::macos;

#[cfg(target_os = "linux")]
use crate::collector::linux;
use crate::collector::{needs_sudo, CollectorManager, CollectorMessage};
use crate::command::{execute_named_command, handle_command_input, handle_notice_enter};
use crate::data::{CollectorRegistration, DashboardStore, DataStore};
use crate::dump::{dump_options_from_args, dump_samples};
use crate::input::InputBindings;
use crate::sudo::prompt_for_sudo;
use crate::tui::{draw_ui, panel_at, AppState};

fn main() -> io::Result<()> {
    if !cfg!(any(target_os = "macos", target_os = "linux")) {
        eprintln!("mbp-info only supports macOS and Linux collectors.");
        return Ok(());
    }

    let args: Vec<String> = env::args().collect();
    let dump_options = match dump_options_from_args(&args) {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            return Ok(());
        }
    };

    let use_sudo = needs_sudo();
    let registrations = register_collectors(use_sudo);

    if let Some(options) = dump_options {
        dump_samples(&options, &registrations)?;
        println!("Wrote sample data to {}", options.path);
        return Ok(());
    }

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
    if let Some(message) = manager.start_defaults(use_sudo, sudo_ready) {
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
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;

    if let Err(error) = result {
        eprintln!("{error}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    rx: mpsc::Receiver<CollectorMessage>,
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
                        if key.code == KeyCode::Char('c')
                            && key
                                .modifiers
                                .contains(crossterm::event::KeyModifiers::CONTROL)
                        {
                            app.clear_command_buffer();
                            continue;
                        }
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

#[cfg(target_os = "macos")]
fn register_collectors(use_sudo: bool) -> Vec<CollectorRegistration> {
    macos::collectors(use_sudo)
}

#[cfg(target_os = "linux")]
fn register_collectors(use_sudo: bool) -> Vec<CollectorRegistration> {
    linux::collectors(use_sudo)
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn register_collectors(_use_sudo: bool) -> Vec<CollectorRegistration> {
    vec![]
}
