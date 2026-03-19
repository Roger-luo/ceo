use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
    Terminal, TerminalOptions, Viewport,
};
use std::io;

// ========================================================================
// Report TUI (existing)
// ========================================================================

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

            let report_text = Text::raw(&app.report_text);
            let report_widget = Paragraph::new(report_text)
                .block(Block::default().borders(Borders::ALL).title(" Report "))
                .wrap(Wrap { trim: false })
                .scroll((app.report_scroll, 0));
            frame.render_widget(report_widget, chunks[0]);

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
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
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
                KeyCode::Char(c) => app.input.push(c),
                KeyCode::Backspace => { app.input.pop(); }
                KeyCode::Up => app.report_scroll = app.report_scroll.saturating_sub(1),
                KeyCode::Down => app.report_scroll = app.report_scroll.saturating_add(1),
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

// ========================================================================
// Config Editor — full-screen tabbed editor
// ========================================================================

use ceo::config::{
    AgentConfig, ClaudeAgentConfig, CodexAgentConfig, GenericAgentConfig,
    Config, RepoConfig, TeamMember,
    TabSpec,
};

/// A single text field in a form tab.
struct FormField {
    /// Config key for `get_field` / `set_field`.
    key: &'static str,
    label: &'static str,
    value: String,
    placeholder: &'static str,
    /// When non-empty, the field is a selector that cycles through these options.
    options: Vec<String>,
}

/// An item in a list tab (repo or team member).
struct ListItem {
    primary: String,
    details: Vec<(&'static str, String)>,
    enabled: bool,
}

/// Content for a tab — either a form or a toggleable list with add.
enum TabContent {
    Form {
        fields: Vec<FormField>,
        selected: usize,
    },
    List {
        items: Vec<ListItem>,
        selected: usize,
        input: String,
        input_cursor: usize,
        focus_input: bool,
        detail_labels: Vec<&'static str>,
        add_placeholder: &'static str,
    },
}

/// What the user is currently doing.
enum Mode {
    /// Navigating tabs and rows.
    Navigate,
    /// Editing a form field's value.
    EditField,
    /// Editing a detail field on a list item: (detail_field_index).
    EditDetail(usize),
}

struct ConfigEditor {
    active_tab: usize,
    tab_names: Vec<&'static str>,
    tabs: Vec<TabContent>,
    mode: Mode,
    edit_buffer: String,
    edit_cursor: usize,
}

impl ConfigEditor {
    fn from_config(config: &Config) -> Self {
        let tab_specs = config.ui_tabs();
        let mut tab_names: Vec<&'static str> = Vec::new();
        let mut tabs: Vec<TabContent> = Vec::new();

        for spec in &tab_specs {
            match spec {
                TabSpec::Form(ft) => {
                    tab_names.push(ft.name);
                    let fields = ft.fields.iter().map(|fs| {
                        let value = config.get_field(fs.key).unwrap_or_default();
                        FormField {
                            key: fs.key,
                            label: fs.label,
                            value,
                            placeholder: fs.placeholder,
                            options: fs.options.clone(),
                        }
                    }).collect();
                    tabs.push(TabContent::Form { fields, selected: 0 });
                }
                TabSpec::List(lt) => {
                    tab_names.push(lt.name);
                    let (items, detail_labels) = list_items_from_config(config, lt.name, &lt.detail_labels);
                    tabs.push(TabContent::List {
                        items,
                        selected: 0,
                        input: String::new(),
                        input_cursor: 0,
                        focus_input: false,
                        detail_labels,
                        add_placeholder: lt.add_placeholder,
                    });
                }
            }
        }

        ConfigEditor {
            active_tab: 0,
            tab_names,
            tabs,
            mode: Mode::Navigate,
            edit_buffer: String::new(),
            edit_cursor: 0,
        }
    }

    /// Fixed inline viewport height. Content scrolls within this.
    fn needed_height(&self) -> u16 {
        16
    }

    fn apply_to_config(&self, config: &mut Config) {
        for (i, (tab, name)) in self.tabs.iter().zip(self.tab_names.iter()).enumerate() {
            match tab {
                TabContent::Form { fields, .. } => {
                    // Agent type must be set first so downstream keys resolve correctly
                    if i == 0 {
                        if let Some(type_field) = fields.iter().find(|f| f.key == "agent.type") {
                            let _ = config.set_field("agent.type", &type_field.value);
                        }
                    }
                    for field in fields {
                        if i == 0 && field.key == "agent.type" {
                            continue; // already handled above
                        }
                        let _ = config.set_field(field.key, &field.value);
                    }
                }
                TabContent::List { items, .. } => {
                    apply_list_to_config(config, name, items);
                }
            }
        }
    }

    fn render(&self, frame: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // tab bar
                Constraint::Min(1),     // content
                Constraint::Length(1),  // status
            ])
            .split(frame.area());

        self.render_tabs(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
        self.render_status(frame, chunks[2]);
    }

    fn render_tabs(&self, frame: &mut ratatui::Frame, area: Rect) {
        let titles: Vec<Line> = self.tab_names.iter()
            .map(|t| Line::from(*t))
            .collect();
        let tabs = Tabs::new(titles)
            .select(self.active_tab)
            .style(Style::default().fg(Color::DarkGray))
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .divider(Span::raw(" │ "))
            .block(Block::default().borders(Borders::ALL).title(" CEO Config "));
        frame.render_widget(tabs, area);
    }

    fn render_content(&self, frame: &mut ratatui::Frame, area: Rect) {
        let inner = Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM);
        let inner_area = inner.inner(area);
        frame.render_widget(inner, area);

        match &self.tabs[self.active_tab] {
            TabContent::Form { fields, selected } => {
                self.render_form(frame, inner_area, fields, *selected);
            }
            TabContent::List { items, selected, input, input_cursor, focus_input, add_placeholder, .. } => {
                self.render_list(frame, inner_area, items, *selected, input, *input_cursor, *focus_input, add_placeholder);
            }
        }
    }

    fn render_form(&self, frame: &mut ratatui::Frame, area: Rect, fields: &[FormField], selected: usize) {
        let mut lines = Vec::new();
        lines.push(Line::raw(""));

        for (i, field) in fields.iter().enumerate() {
            let is_selected = i == selected;
            let is_editing = is_selected && matches!(self.mode, Mode::EditField);

            let marker = if is_selected { "▸ " } else { "  " };
            let label_style = if is_selected {
                Style::default().fg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };

            let label_text = format!("{marker}{:<20}", format!("{}:", field.label));

            if !field.options.is_empty() {
                // Selectable field — show ◂ value ▸
                let display = if field.value.is_empty() { "(none)" } else { &field.value };
                let value_style = if is_selected {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Green)
                };
                let arrow_style = if is_selected {
                    Style::default().fg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                lines.push(Line::from(vec![
                    Span::styled(label_text, label_style),
                    Span::styled("◂ ", arrow_style),
                    Span::styled(display.to_string(), value_style),
                    Span::styled(" ▸", arrow_style),
                ]));
            } else if is_editing {
                let (before, cursor_ch, after) = split_at_cursor(&self.edit_buffer, self.edit_cursor);
                let edit_style = Style::default().fg(Color::Yellow);
                let cursor_style = Style::default().fg(Color::Black).bg(Color::Yellow);
                lines.push(Line::from(vec![
                    Span::styled(label_text, label_style),
                    Span::styled(before, edit_style),
                    Span::styled(cursor_ch, cursor_style),
                    Span::styled(after, edit_style),
                ]));
            } else {
                let (display_value, value_style) = if field.value.is_empty() {
                    (field.placeholder.to_string(), Style::default().fg(Color::DarkGray))
                } else {
                    (field.value.clone(), Style::default().fg(Color::Green))
                };
                lines.push(Line::from(vec![
                    Span::styled(label_text, label_style),
                    Span::styled(display_value, value_style),
                ]));
            }
        }

        let paragraph = Paragraph::new(Text::from(lines));
        frame.render_widget(paragraph, area);
    }

    fn render_list(
        &self,
        frame: &mut ratatui::Frame,
        area: Rect,
        items: &[ListItem],
        selected: usize,
        input: &str,
        input_cursor: usize,
        focus_input: bool,
        add_placeholder: &str,
    ) {
        let available = area.height as usize;
        // Reserve: 1 blank line at top, 1 blank line before input, 1 input line = 3 fixed lines
        let reserved = 3;
        let editing_detail_idx = match self.mode {
            Mode::EditDetail(idx) => Some(idx),
            _ => None,
        };
        // Extra lines for selected item's detail fields when editing
        let detail_lines = if !focus_input && editing_detail_idx.is_some() {
            items.get(selected).map(|item| item.details.len()).unwrap_or(0)
        } else {
            0
        };
        let slots = available.saturating_sub(reserved + detail_lines);

        // Compute visible window of items around the focused index
        let (win_start, win_end) = if items.len() <= slots {
            (0, items.len())
        } else {
            // Center the selected item in the window
            let half = slots / 2;
            let start = if selected <= half {
                0
            } else if selected + slots - half >= items.len() {
                items.len().saturating_sub(slots)
            } else {
                selected - half
            };
            (start, (start + slots).min(items.len()))
        };

        let mut lines: Vec<Line> = Vec::new();

        // Show "↑ N more" if items are hidden above
        if win_start > 0 {
            lines.push(Line::styled(
                format!("   ↑ {} more", win_start),
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            lines.push(Line::raw(""));
        }

        for i in win_start..win_end {
            let item = &items[i];
            let is_selected = !focus_input && i == selected;
            let marker = if is_selected { "▸" } else { " " };
            let check = if item.enabled { "✓" } else { "✗" };

            let (check_style, name_style) = if !item.enabled {
                (Style::default().fg(Color::Red), Style::default().fg(Color::DarkGray))
            } else if is_selected {
                (Style::default().fg(Color::Green), Style::default().fg(Color::Cyan))
            } else {
                (Style::default().fg(Color::Green), Style::default().fg(Color::White))
            };

            let detail_summary: String = item.details.iter()
                .filter(|(_, v)| !v.is_empty())
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join(", ");
            let detail_text = if detail_summary.is_empty() {
                String::new()
            } else {
                format!(" ({detail_summary})")
            };

            lines.push(Line::from(vec![
                Span::styled(format!(" {marker} [{check}] "), check_style),
                Span::styled(&item.primary, name_style),
                Span::styled(detail_text, Style::default().fg(Color::DarkGray)),
            ]));

            // If this item is selected and we're editing details, show detail fields
            if is_selected && editing_detail_idx.is_some() {
                for (di, (label, value)) in item.details.iter().enumerate() {
                    let is_editing_this = editing_detail_idx == Some(di);
                    let di_marker = if is_editing_this { "▸" } else { " " };
                    let label_span = Span::styled(
                        format!("       {di_marker} {label}: "),
                        Style::default().fg(Color::DarkGray),
                    );
                    if is_editing_this {
                        let (before, cursor_ch, after) = split_at_cursor(&self.edit_buffer, self.edit_cursor);
                        let edit_style = Style::default().fg(Color::Yellow);
                        let cursor_style = Style::default().fg(Color::Black).bg(Color::Yellow);
                        lines.push(Line::from(vec![
                            label_span,
                            Span::styled(before, edit_style),
                            Span::styled(cursor_ch, cursor_style),
                            Span::styled(after, edit_style),
                        ]));
                    } else {
                        let display = if value.is_empty() { "(empty)" } else { value.as_str() };
                        lines.push(Line::from(vec![
                            label_span,
                            Span::styled(display, Style::default().fg(Color::DarkGray)),
                        ]));
                    }
                }
            }
        }

        // Show "↓ N more" if items are hidden below
        if win_end < items.len() {
            lines.push(Line::styled(
                format!("   ↓ {} more", items.len() - win_end),
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            lines.push(Line::raw(""));
        }

        // Add input — always visible
        let input_marker = if focus_input { "▸" } else { " " };
        let prefix = Span::styled(format!(" {input_marker}  + "), Style::default().fg(Color::Green));
        if focus_input {
            let text = if input.is_empty() { add_placeholder } else { input };
            let cursor = if input.is_empty() { 0 } else { input_cursor };
            let (before, cursor_ch, after) = split_at_cursor(text, cursor);
            let input_style = Style::default().fg(Color::Cyan);
            let cursor_style = Style::default().fg(Color::Black).bg(Color::Cyan);
            lines.push(Line::from(vec![
                prefix,
                Span::styled(before, input_style),
                Span::styled(cursor_ch, cursor_style),
                Span::styled(after, input_style),
            ]));
        } else {
            let display = if input.is_empty() {
                format!("Add {add_placeholder}")
            } else {
                input.to_string()
            };
            lines.push(Line::from(vec![
                prefix,
                Span::styled(display, Style::default().fg(Color::DarkGray)),
            ]));
        }

        let paragraph = Paragraph::new(Text::from(lines));
        frame.render_widget(paragraph, area);
    }

    fn render_status(&self, frame: &mut ratatui::Frame, area: Rect) {
        let hint = match self.mode {
            Mode::EditField => " ←→ move  Bksp delete  Enter confirm  Esc cancel",
            Mode::EditDetail(_) => " ←→ move  ↑↓ fields  Bksp delete  Enter confirm  Esc done",
            Mode::Navigate => {
                match &self.tabs[self.active_tab] {
                    TabContent::Form { fields, selected } if !fields[*selected].options.is_empty() =>
                        " ←→/Enter cycle  ↑↓ select  Tab/S-Tab tabs  q save & exit",
                    TabContent::Form { .. } => " Tab/S-Tab tabs  ↑↓ select  Enter edit  q save & exit",
                    TabContent::List { focus_input: true, .. } =>
                        " Type to add  Enter add  Esc back",
                    TabContent::List { .. } =>
                        " Tab/S-Tab tabs  ↑↓ select  Space toggle  Enter details  a add  q save & exit",
                }
            }
        };
        let status = Paragraph::new(Line::from(vec![
            Span::styled(hint, Style::default().fg(Color::DarkGray)),
        ]));
        frame.render_widget(status, area);
    }

    /// Returns true if the editor should exit.
    fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        // Ctrl+C always exits without saving
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }

        match self.mode {
            Mode::EditField => self.handle_edit_field(key),
            Mode::EditDetail(di) => self.handle_edit_detail(key, di),
            Mode::Navigate => self.handle_navigate(key),
        }
    }

    fn handle_navigate(&mut self, key: crossterm::event::KeyEvent) -> bool {
        // Check if we're in a list with focus on input
        let in_list_input = matches!(
            &self.tabs[self.active_tab],
            TabContent::List { focus_input: true, .. }
        );

        if in_list_input {
            return self.handle_list_input(key);
        }

        // Check if current field is a selector
        let on_selector = self.current_field_has_options();

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Left if on_selector => self.cycle_option(-1),
            KeyCode::Right if on_selector => self.cycle_option(1),
            KeyCode::Enter if on_selector => self.cycle_option(1),
            KeyCode::Char(' ') if on_selector => self.cycle_option(1),
            KeyCode::Tab => {
                self.active_tab = (self.active_tab + 1) % self.tabs.len();
            }
            KeyCode::BackTab => {
                self.active_tab = if self.active_tab == 0 {
                    self.tabs.len() - 1
                } else {
                    self.active_tab - 1
                };
            }
            KeyCode::Left | KeyCode::Right => {
                // On non-selector form fields or lists: switch tabs
                let delta: i32 = if key.code == KeyCode::Left { -1 } else { 1 };
                let new = self.active_tab as i32 + delta;
                if new >= 0 && (new as usize) < self.tabs.len() {
                    self.active_tab = new as usize;
                }
            }
            KeyCode::Up => self.move_selection(-1),
            KeyCode::Down => self.move_selection(1),
            KeyCode::Enter => self.start_editing(),
            KeyCode::Char(' ') => self.toggle_item(),
            KeyCode::Char('a') => self.focus_add_input(),
            _ => {}
        }
        false
    }

    fn current_field_has_options(&self) -> bool {
        match &self.tabs[self.active_tab] {
            TabContent::Form { fields, selected } => !fields[*selected].options.is_empty(),
            _ => false,
        }
    }

    fn handle_edit_field(&mut self, key: crossterm::event::KeyEvent) -> bool {
        match key.code {
            KeyCode::Enter => {
                self.commit_edit();
                self.mode = Mode::Navigate;
            }
            KeyCode::Esc => {
                self.mode = Mode::Navigate;
            }
            KeyCode::Left => {
                self.edit_cursor = self.edit_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                if self.edit_cursor < self.edit_buffer.len() {
                    self.edit_cursor += 1;
                }
            }
            KeyCode::Char(c) => {
                self.edit_buffer.insert(self.edit_cursor, c);
                self.edit_cursor += 1;
            }
            KeyCode::Backspace => {
                if self.edit_cursor > 0 {
                    self.edit_cursor -= 1;
                    self.edit_buffer.remove(self.edit_cursor);
                }
            }
            _ => {}
        }
        false
    }

    fn handle_edit_detail(&mut self, key: crossterm::event::KeyEvent, detail_idx: usize) -> bool {
        match key.code {
            KeyCode::Enter => {
                self.commit_detail_edit(detail_idx);
                let detail_count = self.current_detail_count();
                if detail_idx + 1 < detail_count {
                    self.start_detail_edit(detail_idx + 1);
                } else {
                    self.mode = Mode::Navigate;
                }
            }
            KeyCode::Esc => {
                self.commit_detail_edit(detail_idx);
                self.mode = Mode::Navigate;
            }
            KeyCode::Left => {
                self.edit_cursor = self.edit_cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                if self.edit_cursor < self.edit_buffer.len() {
                    self.edit_cursor += 1;
                }
            }
            KeyCode::Up => {
                if detail_idx > 0 {
                    self.commit_detail_edit(detail_idx);
                    self.start_detail_edit(detail_idx - 1);
                }
            }
            KeyCode::Down => {
                let detail_count = self.current_detail_count();
                if detail_idx + 1 < detail_count {
                    self.commit_detail_edit(detail_idx);
                    self.start_detail_edit(detail_idx + 1);
                }
            }
            KeyCode::Char(c) => {
                self.edit_buffer.insert(self.edit_cursor, c);
                self.edit_cursor += 1;
            }
            KeyCode::Backspace => {
                if self.edit_cursor > 0 {
                    self.edit_cursor -= 1;
                    self.edit_buffer.remove(self.edit_cursor);
                }
            }
            _ => {}
        }
        false
    }

    fn handle_list_input(&mut self, key: crossterm::event::KeyEvent) -> bool {
        match key.code {
            KeyCode::Esc | KeyCode::Tab => {
                if let TabContent::List { focus_input, .. } = &mut self.tabs[self.active_tab] {
                    *focus_input = false;
                }
            }
            KeyCode::Enter => {
                self.add_list_item();
            }
            KeyCode::Left => {
                if let TabContent::List { input_cursor, .. } = &mut self.tabs[self.active_tab] {
                    *input_cursor = input_cursor.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if let TabContent::List { input, input_cursor, .. } = &mut self.tabs[self.active_tab] {
                    if *input_cursor < input.len() {
                        *input_cursor += 1;
                    }
                }
            }
            KeyCode::Char(c) => {
                if let TabContent::List { input, input_cursor, .. } = &mut self.tabs[self.active_tab] {
                    input.insert(*input_cursor, c);
                    *input_cursor += 1;
                }
            }
            KeyCode::Backspace => {
                if let TabContent::List { input, input_cursor, .. } = &mut self.tabs[self.active_tab] {
                    if *input_cursor > 0 {
                        *input_cursor -= 1;
                        input.remove(*input_cursor);
                    }
                }
            }
            _ => {}
        }
        false
    }

    fn move_selection(&mut self, delta: i32) {
        match &mut self.tabs[self.active_tab] {
            TabContent::Form { fields, selected } => {
                let new = *selected as i32 + delta;
                if new >= 0 && (new as usize) < fields.len() {
                    *selected = new as usize;
                }
            }
            TabContent::List { items, selected, .. } => {
                if items.is_empty() { return; }
                let new = *selected as i32 + delta;
                if new >= 0 && (new as usize) < items.len() {
                    *selected = new as usize;
                }
            }
        }
    }

    fn start_editing(&mut self) {
        match &mut self.tabs[self.active_tab] {
            TabContent::Form { fields, selected } => {
                let sel = *selected;
                if !fields[sel].options.is_empty() {
                    self.cycle_option(1);
                } else {
                    self.edit_buffer = fields[sel].value.clone();
                    self.edit_cursor = self.edit_buffer.len();
                    self.mode = Mode::EditField;
                }
            }
            TabContent::List { items, .. } => {
                if !items.is_empty() {
                    self.start_detail_edit(0);
                }
            }
        }
    }

    fn cycle_option(&mut self, direction: i32) {
        let is_type_field = self.active_tab == 0;
        if let TabContent::Form { fields, selected } = &mut self.tabs[self.active_tab] {
            let field = &mut fields[*selected];
            if field.options.is_empty() { return; }
            let current_idx = field.options.iter()
                .position(|o| o == &field.value)
                .unwrap_or(0) as i32;
            let len = field.options.len() as i32;
            let next_idx = ((current_idx + direction) % len + len) % len;
            field.value = field.options[next_idx as usize].clone();
        }
        // When Type changes, update Command default and rebuild Models tab
        if is_type_field {
            if let TabContent::Form { fields, .. } = &mut self.tabs[0] {
                let agent_type = fields[0].value.clone();
                let default_cmd = match agent_type.as_str() {
                    "codex" => "codex",
                    "generic" => "",
                    _ => "claude",
                };
                // Only update command if it's still a default value
                let cmd = &fields[1].value;
                if cmd == "claude" || cmd == "codex" || cmd.is_empty() {
                    fields[1].value = default_cmd.to_string();
                }
            }
            self.rebuild_models_tab();
        }
    }

    fn rebuild_models_tab(&mut self) {
        let agent_type = if let TabContent::Form { fields, .. } = &self.tabs[0] {
            fields[0].value.clone()
        } else {
            return;
        };

        // Preserve shared model values from current Models tab
        let preserved: std::collections::HashMap<&str, String> =
            if let TabContent::Form { fields, .. } = &self.tabs[1] {
                fields.iter().map(|f| (f.key, f.value.clone())).collect()
            } else {
                std::collections::HashMap::new()
            };

        // Build a temporary config of the new agent type to get the schema
        let tmp_agent: AgentConfig = match agent_type.as_str() {
            "codex" => AgentConfig::Codex(CodexAgentConfig::default()),
            "generic" => AgentConfig::Generic(GenericAgentConfig::default()),
            _ => AgentConfig::Claude(ClaudeAgentConfig::default()),
        };
        let tmp_config = Config {
            agent: tmp_agent,
            repos: vec![],
            team: vec![],
            project: None,
            editor: None,
            summary_length: None,
            batch_size: None,
            concurrency: None,
            slack: None,
        };

        // Find the Models tab spec from the schema
        let tab_specs = tmp_config.ui_tabs();
        let models_spec = tab_specs.iter().find(|t| matches!(t, TabSpec::Form(ft) if ft.name == "Models"));
        if let Some(TabSpec::Form(ft)) = models_spec {
            let fields = ft.fields.iter().map(|fs| {
                let value = preserved.get(fs.key).cloned().unwrap_or_default();
                FormField {
                    key: fs.key,
                    label: fs.label,
                    value,
                    placeholder: fs.placeholder,
                    options: fs.options.clone(),
                }
            }).collect();
            self.tabs[1] = TabContent::Form { fields, selected: 0 };
        }
    }

    fn commit_edit(&mut self) {
        if let TabContent::Form { fields, selected } = &mut self.tabs[self.active_tab] {
            fields[*selected].value = self.edit_buffer.clone();
        }
    }

    fn start_detail_edit(&mut self, detail_idx: usize) {
        if let TabContent::List { items, selected, .. } = &self.tabs[self.active_tab] {
            if let Some(item) = items.get(*selected) {
                if let Some((_, value)) = item.details.get(detail_idx) {
                    self.edit_buffer = value.clone();
                    self.edit_cursor = self.edit_buffer.len();
                    self.mode = Mode::EditDetail(detail_idx);
                }
            }
        }
    }

    fn commit_detail_edit(&mut self, detail_idx: usize) {
        if let TabContent::List { items, selected, .. } = &mut self.tabs[self.active_tab] {
            if let Some(item) = items.get_mut(*selected) {
                if let Some((_, value)) = item.details.get_mut(detail_idx) {
                    *value = self.edit_buffer.clone();
                }
            }
        }
    }

    fn current_detail_count(&self) -> usize {
        if let TabContent::List { items, selected, .. } = &self.tabs[self.active_tab] {
            items.get(*selected).map(|item| item.details.len()).unwrap_or(0)
        } else {
            0
        }
    }

    fn toggle_item(&mut self) {
        if let TabContent::List { items, selected, focus_input, .. } = &mut self.tabs[self.active_tab] {
            if !*focus_input {
                if let Some(item) = items.get_mut(*selected) {
                    item.enabled = !item.enabled;
                }
            }
        }
    }

    fn focus_add_input(&mut self) {
        if let TabContent::List { focus_input, .. } = &mut self.tabs[self.active_tab] {
            *focus_input = true;
        }
    }

    fn add_list_item(&mut self) {
        if let TabContent::List { items, input, input_cursor, selected, detail_labels, .. } = &mut self.tabs[self.active_tab] {
            let trimmed = input.trim().to_string();
            if !trimmed.is_empty() {
                let details: Vec<(&'static str, String)> = detail_labels.iter()
                    .map(|label| (*label, String::new()))
                    .collect();
                items.push(ListItem {
                    primary: trimmed,
                    details,
                    enabled: true,
                });
                input.clear();
                *input_cursor = 0;
                *selected = items.len() - 1;
            }
        }
    }
}

/// Split a string at cursor position into (before, cursor_char, after).
/// If cursor is at the end, cursor_char is " " (visible block cursor).
fn split_at_cursor(s: &str, cursor: usize) -> (String, String, String) {
    let before = s[..cursor].to_string();
    if cursor < s.len() {
        let cursor_ch = s[cursor..cursor + 1].to_string();
        let after = s[cursor + 1..].to_string();
        (before, cursor_ch, after)
    } else {
        (before, " ".to_string(), String::new())
    }
}

/// Build list items from the config for a named list tab.
fn list_items_from_config(config: &Config, tab_name: &str, detail_labels: &[&'static str]) -> (Vec<ListItem>, Vec<&'static str>) {
    match tab_name {
        "Repos" => {
            let items = config.repos.iter().map(|r| {
                let labels = r.labels_required.join(", ");
                let branches = r.branches.join(", ");
                ListItem {
                    primary: r.name.clone(),
                    details: detail_labels.iter().zip([labels, branches])
                        .map(|(l, v)| (*l, v))
                        .collect(),
                    enabled: true,
                }
            }).collect();
            (items, detail_labels.to_vec())
        }
        "Team" => {
            let items = config.team.iter().map(|m| {
                ListItem {
                    primary: format!("@{}", m.github),
                    details: detail_labels.iter().zip([m.name.clone(), m.role.clone()])
                        .map(|(l, v)| (*l, v))
                        .collect(),
                    enabled: true,
                }
            }).collect();
            (items, detail_labels.to_vec())
        }
        _ => (vec![], detail_labels.to_vec()),
    }
}

/// Apply list tab items back to the config.
fn apply_list_to_config(config: &mut Config, tab_name: &str, items: &[ListItem]) {
    match tab_name {
        "Repos" => {
            config.repos = items.iter()
                .filter(|item| item.enabled)
                .map(|item| {
                    let labels: Vec<String> = item.details.first()
                        .map(|(_, v)| v.as_str()).unwrap_or("")
                        .split(',').map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()).collect();
                    let branches: Vec<String> = item.details.get(1)
                        .map(|(_, v)| v.as_str()).unwrap_or("")
                        .split(',').map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()).collect();
                    RepoConfig { name: item.primary.clone(), labels_required: labels, branches }
                }).collect();
        }
        "Team" => {
            config.team = items.iter()
                .filter(|item| item.enabled)
                .map(|item| {
                    let github = item.primary.trim_start_matches('@').to_string();
                    let name = item.details.first().map(|(_, v)| v.clone()).unwrap_or_default();
                    let role = item.details.get(1).map(|(_, v)| v.clone()).unwrap_or_default();
                    TeamMember { github, name, role }
                }).collect();
        }
        _ => {}
    }
}


/// Run the inline tabbed config editor. Modifies config in place.
pub fn run_config_editor(config: &mut Config) -> Result<()> {
    let mut editor = ConfigEditor::from_config(config);
    let height = editor.needed_height();

    enable_raw_mode()?;
    let stdout = io::stdout();
    let backend = CrosstermBackend::new(stdout);
    let options = TerminalOptions {
        viewport: Viewport::Inline(height),
    };
    let mut terminal = Terminal::with_options(backend, options)?;

    loop {
        terminal.draw(|frame| editor.render(frame))?;

        if let Event::Key(key) = event::read()? {
            if editor.handle_key(key) {
                break;
            }
        }
    }

    editor.apply_to_config(config);

    // Clear the inline viewport so the panel doesn't linger in terminal history
    terminal.clear()?;

    disable_raw_mode()?;
    Ok(())
}
