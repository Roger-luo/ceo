//! Report TUI state and logic — testable without a real terminal.
//!
//! The event loop and terminal setup live in the binary crate's `tui.rs`.

use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Available slash commands: (name, description).
pub const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show available commands"),
    ("/quit", "Exit interactive mode"),
    ("/top", "Scroll to top of report"),
    ("/bottom", "Scroll to bottom of report"),
    ("/search", "Search report text — /search <query>"),
];

/// What the event loop should do after a key press.
pub enum Action {
    Continue,
    Quit,
}

impl Action {
    pub fn should_quit(&self) -> bool {
        matches!(self, Action::Quit)
    }
}

pub struct TuiApp {
    pub report_text: String,
    pub input: String,
    pub input_cursor: usize,
    pub output_lines: Vec<String>,
    pub report_scroll: u16,
    pub should_quit: bool,
    pub report_height: u16,
    pub completions: Vec<String>,
    pub completion_idx: Option<usize>,
    pub search_query: String,
}

impl TuiApp {
    pub fn new(report_text: String) -> Self {
        Self {
            report_text,
            input: String::new(),
            input_cursor: 0,
            output_lines: vec!["Type /help for commands. Press Esc to quit.".to_string()],
            report_scroll: 0,
            should_quit: false,
            report_height: 0,
            completions: Vec::new(),
            completion_idx: None,
            search_query: String::new(),
        }
    }

    // --- Rendering -----------------------------------------------------------

    pub fn render(&mut self, frame: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(70),
                Constraint::Percentage(30),
                Constraint::Length(1),  // status bar
            ])
            .split(frame.area());

        self.report_height = chunks[0].height.saturating_sub(2);

        // Report pane
        let title = if self.search_query.is_empty() {
            " Report ".to_string()
        } else {
            format!(" Report [searching: {}] ", self.search_query)
        };
        let report_widget = Paragraph::new(Text::raw(&self.report_text))
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: false })
            .scroll((self.report_scroll, 0));
        frame.render_widget(report_widget, chunks[0]);

        // Command pane
        let mut repl_lines: Vec<Line> = self.output_lines
            .iter()
            .map(|l| Line::raw(l.as_str()))
            .collect();

        // Prompt with cursor
        let cursor = self.input_cursor.min(self.input.len());
        let before = &self.input[..cursor];
        let (cursor_ch, after) = if cursor < self.input.len() {
            (&self.input[cursor..cursor + 1], &self.input[cursor + 1..])
        } else {
            (" ", "")
        };
        repl_lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Green)),
            Span::raw(before.to_string()),
            Span::styled(cursor_ch.to_string(), Style::default().fg(Color::Black).bg(Color::Green)),
            Span::raw(after.to_string()),
        ]));

        // Completion menu — vertical list, one command per line with description
        if !self.completions.is_empty() {
            for (i, name) in self.completions.iter().enumerate() {
                let desc = SLASH_COMMANDS.iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, d)| *d)
                    .unwrap_or("");
                let is_selected = self.completion_idx == Some(i);
                let (name_style, desc_style) = if is_selected {
                    (
                        Style::default().fg(Color::Black).bg(Color::Cyan),
                        Style::default().fg(Color::Black).bg(Color::Cyan),
                    )
                } else {
                    (
                        Style::default().fg(Color::Cyan),
                        Style::default().fg(Color::DarkGray),
                    )
                };
                repl_lines.push(Line::from(vec![
                    Span::styled(format!("  {name:<12}"), name_style),
                    Span::styled(format!(" {desc}"), desc_style),
                ]));
            }
        }

        let repl_widget = Paragraph::new(Text::from(repl_lines))
            .block(Block::default().borders(Borders::ALL).title(" Commands "))
            .wrap(Wrap { trim: false });
        frame.render_widget(repl_widget, chunks[1]);

        // Status bar
        let hint = if !self.completions.is_empty() {
            " Tab cycle  Enter select  Esc dismiss"
        } else if !self.search_query.is_empty() {
            " Esc clear search  PgUp/PgDn scroll  /search <query>"
        } else {
            " Up/Dn scroll  PgUp/PgDn page  /command  Esc quit"
        };
        let status = Paragraph::new(Line::styled(hint, Style::default().fg(Color::DarkGray)));
        frame.render_widget(status, chunks[2]);
    }

    // --- Input handling ------------------------------------------------------

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Action {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Action::Quit;
        }

        if key.code == KeyCode::Tab && !self.completions.is_empty() {
            self.cycle_completion();
            return Action::Continue;
        }

        if key.code != KeyCode::Enter && key.code != KeyCode::Tab {
            self.completions.clear();
            self.completion_idx = None;
        }

        match key.code {
            KeyCode::Enter => self.submit_input(),

            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.insert(self.input_cursor, c);
                self.input_cursor += c.len_utf8();
                self.update_completions();
                Action::Continue
            }
            KeyCode::Backspace => {
                if self.input_cursor > 0 {
                    self.input_cursor -= 1;
                    self.input.remove(self.input_cursor);
                    self.update_completions();
                }
                Action::Continue
            }

            KeyCode::Left => {
                self.input_cursor = self.input_cursor.saturating_sub(1);
                Action::Continue
            }
            KeyCode::Right => {
                if self.input_cursor < self.input.len() {
                    self.input_cursor += 1;
                }
                Action::Continue
            }

            KeyCode::Home if self.input.is_empty() => {
                self.report_scroll = 0;
                Action::Continue
            }
            KeyCode::End if self.input.is_empty() => {
                self.scroll_to_bottom();
                Action::Continue
            }

            KeyCode::Up => {
                self.report_scroll = self.report_scroll.saturating_sub(1);
                Action::Continue
            }
            KeyCode::Down => {
                self.scroll_down(1);
                Action::Continue
            }
            KeyCode::PageUp => {
                let page = self.report_height.max(1);
                self.report_scroll = self.report_scroll.saturating_sub(page);
                Action::Continue
            }
            KeyCode::PageDown => {
                let page = self.report_height.max(1);
                self.scroll_down(page);
                Action::Continue
            }

            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.clear();
                self.input_cursor = 0;
                self.completions.clear();
                self.completion_idx = None;
                Action::Continue
            }
            KeyCode::Char('a') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_cursor = 0;
                Action::Continue
            }
            KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input_cursor = self.input.len();
                Action::Continue
            }

            KeyCode::Esc => {
                if !self.search_query.is_empty() {
                    self.search_query.clear();
                    Action::Continue
                } else {
                    Action::Quit
                }
            }

            _ => Action::Continue,
        }
    }

    // --- Scroll helpers ------------------------------------------------------

    fn report_line_count(&self) -> u16 {
        self.report_text.lines().count().max(1) as u16
    }

    fn max_scroll(&self) -> u16 {
        self.report_line_count().saturating_sub(self.report_height)
    }

    fn scroll_down(&mut self, lines: u16) {
        self.report_scroll = self.report_scroll.saturating_add(lines).min(self.max_scroll());
    }

    fn scroll_to_bottom(&mut self) {
        self.report_scroll = self.max_scroll();
    }

    // --- Command execution ---------------------------------------------------

    fn submit_input(&mut self) -> Action {
        let cmd = self.input.clone();
        self.input.clear();
        self.input_cursor = 0;
        self.completions.clear();
        self.completion_idx = None;

        if cmd.is_empty() {
            return Action::Continue;
        }

        self.output_lines.push(format!("> {cmd}"));

        if let Some(text) = self.execute_command(&cmd) {
            for line in text.lines() {
                self.output_lines.push(line.to_string());
            }
        }

        if self.should_quit { Action::Quit } else { Action::Continue }
    }

    fn execute_command(&mut self, cmd: &str) -> Option<String> {
        let trimmed = cmd.trim();

        if trimmed.starts_with('/') {
            let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
            let name = parts[0];
            let arg = parts.get(1).unwrap_or(&"").trim();
            return match name {
                "/help" => {
                    let mut help = String::from("Commands:\n");
                    for (cmd, desc) in SLASH_COMMANDS {
                        help.push_str(&format!("  {cmd:<12} {desc}\n"));
                    }
                    help.push_str("\nType / then Tab to autocomplete. Esc to quit.");
                    Some(help)
                }
                "/quit" => {
                    self.should_quit = true;
                    None
                }
                "/top" => {
                    self.report_scroll = 0;
                    Some("Scrolled to top.".to_string())
                }
                "/bottom" => {
                    self.scroll_to_bottom();
                    Some("Scrolled to bottom.".to_string())
                }
                "/search" => {
                    if arg.is_empty() {
                        self.search_query.clear();
                        Some("Search cleared.".to_string())
                    } else {
                        self.search_query = arg.to_string();
                        if let Some(line_idx) = self.find_search_line() {
                            self.report_scroll = line_idx as u16;
                            Some(format!("Found at line {line_idx}."))
                        } else {
                            Some(format!("No match for \"{arg}\"."))
                        }
                    }
                }
                _ => Some(format!("Unknown command: {name}. Type /help for commands.")),
            };
        }

        match trimmed {
            "help" => self.execute_command("/help"),
            "quit" | "exit" => {
                self.should_quit = true;
                None
            }
            other => Some(format!("Unknown command: {other}. Type /help for commands.")),
        }
    }

    fn find_search_line(&self) -> Option<usize> {
        let query = self.search_query.to_lowercase();
        self.report_text.lines()
            .enumerate()
            .find(|(_, line)| line.to_lowercase().contains(&query))
            .map(|(i, _)| i)
    }

    // --- Tab completion ------------------------------------------------------

    fn update_completions(&mut self) {
        if !self.input.starts_with('/') {
            self.completions.clear();
            self.completion_idx = None;
            return;
        }
        let prefix = &self.input;
        self.completions = SLASH_COMMANDS.iter()
            .filter(|(name, _)| name.starts_with(prefix) && *name != prefix)
            .map(|(name, _)| name.to_string())
            .collect();
        self.completion_idx = None;
    }

    fn cycle_completion(&mut self) {
        if self.completions.is_empty() { return; }
        let idx = match self.completion_idx {
            Some(i) => (i + 1) % self.completions.len(),
            None => 0,
        };
        self.completion_idx = Some(idx);
        self.input = self.completions[idx].clone();
        self.input_cursor = self.input.len();
    }
}
