use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::io;

pub struct TuiApp {
    pub report_text: String,
    pub input: String,
    pub output_lines: Vec<String>,
    pub report_scroll: u16,
    pub should_quit: bool,
}

impl TuiApp {
    pub fn new(report_text: String) -> Self {
        Self {
            report_text,
            input: String::new(),
            output_lines: vec!["Type `help` for commands, `quit` to exit.".to_string()],
            report_scroll: 0,
            should_quit: false,
        }
    }

    pub fn handle_command(&mut self, cmd: &str) -> Option<String> {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        match parts.first().copied() {
            Some("help") => Some(
                "Commands:\n  refresh  — re-fetch report\n  show #N  — show issue detail\n  \
                 analyze #N — re-run agent on issue\n  repos — list repos\n  quit — exit"
                    .to_string(),
            ),
            Some("quit") | Some("exit") => {
                self.should_quit = true;
                None
            }
            Some("repos") => Some("(repos command — not yet wired up)".to_string()),
            Some("refresh") => Some("(refresh — not yet wired up)".to_string()),
            Some("show") => Some("(show — not yet wired up)".to_string()),
            Some("analyze") => Some("(analyze — not yet wired up)".to_string()),
            Some(other) => Some(format!("Unknown command: {other}. Type `help` for commands.")),
            None => None,
        }
    }
}

pub fn run_tui(report_text: String) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new(report_text);

    loop {
        terminal.draw(|frame| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(65), Constraint::Percentage(35)])
                .split(frame.area());

            // Top pane: report
            let report_text = Text::raw(&app.report_text);
            let report_widget = Paragraph::new(report_text)
                .block(Block::default().borders(Borders::ALL).title(" Report "))
                .wrap(Wrap { trim: false })
                .scroll((app.report_scroll, 0));
            frame.render_widget(report_widget, chunks[0]);

            // Bottom pane: REPL
            let mut repl_lines: Vec<Line> = app
                .output_lines
                .iter()
                .map(|l| Line::raw(l.as_str()))
                .collect();
            repl_lines.push(Line::styled(
                format!("> {}_", app.input),
                Style::default().fg(Color::Green),
            ));
            let repl_widget = Paragraph::new(Text::from(repl_lines))
                .block(Block::default().borders(Borders::ALL).title(" Commands "))
                .wrap(Wrap { trim: false });
            frame.render_widget(repl_widget, chunks[1]);
        })?;

        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    break;
                }
                KeyCode::Enter => {
                    let cmd = app.input.clone();
                    app.input.clear();
                    app.output_lines.push(format!("> {cmd}"));
                    if let Some(response) = app.handle_command(&cmd) {
                        for line in response.lines() {
                            app.output_lines.push(line.to_string());
                        }
                    }
                    if app.should_quit {
                        break;
                    }
                }
                KeyCode::Char(c) => {
                    app.input.push(c);
                }
                KeyCode::Backspace => {
                    app.input.pop();
                }
                KeyCode::Up => {
                    app.report_scroll = app.report_scroll.saturating_sub(1);
                }
                KeyCode::Down => {
                    app.report_scroll = app.report_scroll.saturating_add(1);
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
