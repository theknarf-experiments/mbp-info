use std::collections::HashMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum KeyToken {
    Char(char),
    Ctrl(char),
    Leader,
    Up,
    Down,
    Left,
    Right,
    Esc,
    Enter,
    Tab,
    Backspace,
}

pub const COMMANDS: &[&str] = &[
    "q",
    "quit",
    "help",
    "collectors",
    "dashboards",
    "leader_set",
    "leader_unset",
    "dashboard_new",
    "dashboard_delete",
    "dashboard_next",
    "dashboard_prev",
    "dashboard_goto",
    "collector",
    "reauth",
    "map",
    "unmap",
    "maps",
    "jump_to_first_row",
    "jump_to_last_row",
    "cursor_up",
    "cursor_down",
    "panel_navigate_left",
    "panel_navigate_right",
    "panel_navigate_up",
    "panel_navigate_down",
];

const MAPPABLE_COMMANDS: &[&str] = &[
    "quit",
    "help",
    "collectors",
    "dashboards",
    "dashboard_new",
    "dashboard_delete",
    "dashboard_next",
    "dashboard_prev",
    "maps",
    "jump_to_first_row",
    "jump_to_last_row",
    "cursor_up",
    "cursor_down",
    "panel_navigate_left",
    "panel_navigate_right",
    "panel_navigate_up",
    "panel_navigate_down",
];

const NO_ARG_COMMANDS: &[&str] = &[
    "quit",
    "help",
    "collectors",
    "dashboards",
    "dashboard_new",
    "dashboard_delete",
    "dashboard_next",
    "dashboard_prev",
    "maps",
    "jump_to_first_row",
    "jump_to_last_row",
    "cursor_up",
    "cursor_down",
    "panel_navigate_left",
    "panel_navigate_right",
    "panel_navigate_up",
    "panel_navigate_down",
];

pub struct InputBindings {
    map: HashMap<Vec<KeyToken>, String>,
    pending: Vec<KeyToken>,
    leader: Option<char>,
}

impl InputBindings {
    pub fn new() -> Self {
        let mut bindings = Self {
            map: HashMap::new(),
            pending: Vec::new(),
            leader: Some(','),
        };
        bindings.load_defaults();
        bindings
    }

    pub fn resolve(&mut self, event: KeyEvent) -> Option<String> {
        let Some(token) = key_event_to_token(event) else {
            self.pending.clear();
            return None;
        };

        let mut attempted = self.pending.clone();
        attempted.push(token.clone());

        if let Some(command) = self.lookup_exact(&attempted) {
            if !self.has_prefix_longer(&attempted) {
                self.pending.clear();
                return Some(command);
            }
        }

        if self.has_prefix(&attempted) {
            self.pending = attempted;
            return None;
        }

        self.pending.clear();
        self.lookup_exact(&[token])
    }

    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }

    pub fn bind(&mut self, sequence: &str, command: &str) -> Result<(), String> {
        let canonical =
            canonical_command(command).ok_or_else(|| format!("Unknown command: {command}"))?;
        if !is_mappable_command(canonical) {
            return Err(format!("Command not mappable: {command}"));
        }
        let tokens = parse_key_sequence(sequence)?;
        if tokens.is_empty() {
            return Err("Mapping sequence is empty.".to_string());
        }
        self.map.insert(tokens, canonical.to_string());
        Ok(())
    }

    pub fn unbind(&mut self, sequence: &str) -> Result<(), String> {
        let tokens = parse_key_sequence(sequence)?;
        if self.map.remove(&tokens).is_some() {
            Ok(())
        } else {
            Err(format!("No mapping for: {sequence}"))
        }
    }

    pub fn is_command_known(command: &str) -> bool {
        canonical_command(command).is_some()
    }

    pub fn is_no_arg_command(command: &str) -> bool {
        let Some(canonical) = canonical_command(command) else {
            return false;
        };
        NO_ARG_COMMANDS.contains(&canonical)
    }

    pub fn mapping_rows(&self) -> Vec<Vec<String>> {
        let mut entries: Vec<(String, String)> = self
            .map
            .iter()
            .map(|(sequence, command)| (format_sequence(sequence), command.clone()))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        if entries.is_empty() {
            return vec![vec!["No key mappings.".to_string()]];
        }

        let mut rows = Vec::with_capacity(entries.len() + 1);
        rows.push(vec!["Keys".to_string(), "Command".to_string()]);
        rows.extend(
            entries
                .into_iter()
                .map(|(sequence, command)| vec![sequence, command]),
        );
        rows
    }

    pub fn set_leader(&mut self, leader: Option<char>) {
        self.leader = leader;
        self.pending.clear();
    }

    fn lookup_exact(&self, tokens: &[KeyToken]) -> Option<String> {
        if let Some(command) = self.map.get(tokens) {
            return Some(command.clone());
        }
        self.map
            .iter()
            .find(|(sequence, _)| matches_sequence(tokens, sequence, self.leader))
            .map(|(_, command)| command.clone())
    }

    fn has_prefix(&self, tokens: &[KeyToken]) -> bool {
        self.map
            .keys()
            .any(|mapped| is_prefix_with_leader(tokens, mapped, self.leader))
    }

    fn has_prefix_longer(&self, tokens: &[KeyToken]) -> bool {
        self.map.keys().any(|mapped| {
            mapped.len() > tokens.len() && is_prefix_with_leader(tokens, mapped, self.leader)
        })
    }

    fn load_defaults(&mut self) {
        self.insert_tokens(vec![KeyToken::Char('j')], "cursor_down");
        self.insert_tokens(vec![KeyToken::Down], "cursor_down");
        self.insert_tokens(vec![KeyToken::Char('k')], "cursor_up");
        self.insert_tokens(vec![KeyToken::Up], "cursor_up");
        self.insert_tokens(
            vec![KeyToken::Char('g'), KeyToken::Char('g')],
            "jump_to_first_row",
        );
        self.insert_tokens(vec![KeyToken::Char('G')], "jump_to_last_row");
        self.insert_tokens(
            vec![KeyToken::Ctrl('w'), KeyToken::Char('h')],
            "panel_navigate_left",
        );
        self.insert_tokens(
            vec![KeyToken::Ctrl('w'), KeyToken::Left],
            "panel_navigate_left",
        );
        self.insert_tokens(
            vec![KeyToken::Ctrl('w'), KeyToken::Char('j')],
            "panel_navigate_down",
        );
        self.insert_tokens(
            vec![KeyToken::Ctrl('w'), KeyToken::Down],
            "panel_navigate_down",
        );
        self.insert_tokens(
            vec![KeyToken::Ctrl('w'), KeyToken::Char('k')],
            "panel_navigate_up",
        );
        self.insert_tokens(vec![KeyToken::Ctrl('w'), KeyToken::Up], "panel_navigate_up");
        self.insert_tokens(
            vec![KeyToken::Ctrl('w'), KeyToken::Char('l')],
            "panel_navigate_right",
        );
        self.insert_tokens(
            vec![KeyToken::Ctrl('w'), KeyToken::Right],
            "panel_navigate_right",
        );
        self.insert_tokens(vec![KeyToken::Ctrl('c')], "quit");

        self.insert_tokens(
            vec![KeyToken::Leader, KeyToken::Char('n')],
            "dashboard_next",
        );
        self.insert_tokens(
            vec![KeyToken::Leader, KeyToken::Char('p')],
            "dashboard_prev",
        );
        self.insert_tokens(vec![KeyToken::Leader, KeyToken::Char('c')], "dashboard_new");
        self.insert_tokens(vec![KeyToken::Leader, KeyToken::Char('h')], "help");
        self.insert_tokens(vec![KeyToken::Leader, KeyToken::Char('q')], "quit");
    }

    fn insert_tokens(&mut self, tokens: Vec<KeyToken>, command: &str) {
        if let Some(canonical) = canonical_command(command) {
            if is_mappable_command(canonical) {
                self.map.insert(tokens, canonical.to_string());
            }
        }
    }
}

pub fn parse_key_sequence(input: &str) -> Result<Vec<KeyToken>, String> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '<' {
            let mut token = String::new();
            let mut closed = false;
            for next in chars.by_ref() {
                if next == '>' {
                    closed = true;
                    break;
                }
                token.push(next);
            }
            if !closed {
                return Err("Unclosed <...> token in key sequence.".to_string());
            }
            tokens.push(parse_bracket_token(&token)?);
        } else {
            tokens.push(KeyToken::Char(ch));
        }
    }

    Ok(tokens)
}

fn parse_bracket_token(token: &str) -> Result<KeyToken, String> {
    let lower = token.to_lowercase();
    if let Some(rest) = lower.strip_prefix("c-") {
        let mut chars = rest.chars();
        let Some(ch) = chars.next() else {
            return Err("Invalid ctrl token.".to_string());
        };
        if chars.next().is_some() {
            return Err("Ctrl token must be a single character.".to_string());
        }
        return Ok(KeyToken::Ctrl(ch));
    }

    match lower.as_str() {
        "leader" => Ok(KeyToken::Leader),
        "up" => Ok(KeyToken::Up),
        "down" => Ok(KeyToken::Down),
        "left" => Ok(KeyToken::Left),
        "right" => Ok(KeyToken::Right),
        "esc" | "escape" => Ok(KeyToken::Esc),
        "enter" | "cr" => Ok(KeyToken::Enter),
        "tab" => Ok(KeyToken::Tab),
        "bs" | "backspace" => Ok(KeyToken::Backspace),
        _ => Err(format!("Unknown key token: <{token}>")),
    }
}

fn key_event_to_token(event: KeyEvent) -> Option<KeyToken> {
    match event.code {
        KeyCode::Char(ch) => {
            if event.modifiers.contains(KeyModifiers::CONTROL) {
                Some(KeyToken::Ctrl(ch.to_ascii_lowercase()))
            } else {
                Some(KeyToken::Char(ch))
            }
        }
        KeyCode::Up => Some(KeyToken::Up),
        KeyCode::Down => Some(KeyToken::Down),
        KeyCode::Left => Some(KeyToken::Left),
        KeyCode::Right => Some(KeyToken::Right),
        KeyCode::Esc => Some(KeyToken::Esc),
        KeyCode::Enter => Some(KeyToken::Enter),
        KeyCode::Tab => Some(KeyToken::Tab),
        KeyCode::Backspace => Some(KeyToken::Backspace),
        _ => None,
    }
}

fn is_prefix_with_leader(prefix: &[KeyToken], sequence: &[KeyToken], leader: Option<char>) -> bool {
    if sequence.len() < prefix.len() {
        return false;
    }
    sequence[..prefix.len()]
        .iter()
        .zip(prefix.iter())
        .all(|(mapped, input)| token_matches(input, mapped, leader))
}

fn matches_sequence(input: &[KeyToken], sequence: &[KeyToken], leader: Option<char>) -> bool {
    if input.len() != sequence.len() {
        return false;
    }
    input
        .iter()
        .zip(sequence.iter())
        .all(|(input_token, mapped_token)| token_matches(input_token, mapped_token, leader))
}

fn token_matches(input: &KeyToken, mapped: &KeyToken, leader: Option<char>) -> bool {
    match mapped {
        KeyToken::Leader => leader
            .map(|leader| *input == KeyToken::Char(leader))
            .unwrap_or(false),
        _ => input == mapped,
    }
}

fn canonical_command(command: &str) -> Option<&'static str> {
    match command {
        "q" | "quit" => Some("quit"),
        "help" => Some("help"),
        "collectors" => Some("collectors"),
        "dashboards" => Some("dashboards"),
        "dashboard_new" => Some("dashboard_new"),
        "dashboard_delete" => Some("dashboard_delete"),
        "dashboard_next" => Some("dashboard_next"),
        "dashboard_prev" => Some("dashboard_prev"),
        "dashboard_goto" => Some("dashboard_goto"),
        "leader_set" => Some("leader_set"),
        "leader_unset" => Some("leader_unset"),
        "collector" => Some("collector"),
        "reauth" => Some("reauth"),
        "map" => Some("map"),
        "unmap" => Some("unmap"),
        "maps" => Some("maps"),
        "jump_to_first_row" => Some("jump_to_first_row"),
        "jump_to_last_row" => Some("jump_to_last_row"),
        "cursor_up" => Some("cursor_up"),
        "cursor_down" => Some("cursor_down"),
        "panel_navigate_left" => Some("panel_navigate_left"),
        "panel_navigate_right" => Some("panel_navigate_right"),
        "panel_navigate_up" => Some("panel_navigate_up"),
        "panel_navigate_down" => Some("panel_navigate_down"),
        _ => None,
    }
}

fn is_mappable_command(command: &str) -> bool {
    MAPPABLE_COMMANDS.contains(&command)
}

fn format_sequence(sequence: &[KeyToken]) -> String {
    let mut output = String::new();
    for token in sequence {
        output.push_str(&format_token(token));
    }
    output
}

fn format_token(token: &KeyToken) -> String {
    match token {
        KeyToken::Char(ch) => ch.to_string(),
        KeyToken::Ctrl(ch) => format!("<c-{ch}>"),
        KeyToken::Leader => "<leader>".to_string(),
        KeyToken::Up => "<up>".to_string(),
        KeyToken::Down => "<down>".to_string(),
        KeyToken::Left => "<left>".to_string(),
        KeyToken::Right => "<right>".to_string(),
        KeyToken::Esc => "<esc>".to_string(),
        KeyToken::Enter => "<enter>".to_string(),
        KeyToken::Tab => "<tab>".to_string(),
        KeyToken::Backspace => "<bs>".to_string(),
    }
}
