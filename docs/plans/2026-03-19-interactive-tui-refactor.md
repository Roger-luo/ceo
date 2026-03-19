# Interactive TUI Refactor Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refactor the interactive report viewer to support proper keyboard navigation, slash commands with tab completion, and testable architecture.

**Architecture:** Separate `TuiApp` state management from rendering. The app struct holds all state and exposes `handle_key(KeyEvent) -> Action` for input and `render(&self, frame: &mut Frame)` for drawing. The event loop in `run_tui` just connects the two. Slash commands are a simple registry mapping names to handler closures. Tests use `TestBackend` + `insta` for rendering snapshots and plain unit tests for state transitions.

**Tech Stack:** ratatui 0.30 (`TestBackend`, `Buffer`, `Paragraph`), crossterm 0.29 (`KeyEvent`, `KeyCode`), insta (snapshot testing)

---

## File Structure

| File | Responsibility |
|---|---|
| `src/tui.rs` (modify, lines 1-130) | `TuiApp` struct, state management, `handle_key`, `render`, `handle_command`, slash command registry, tab completion. The config editor section (lines 132+) is untouched. |
| `tests/tui_test.rs` (create) | Unit tests for state transitions + `TestBackend`/`insta` rendering snapshots |
| `Cargo.toml` (modify) | No new deps needed — `ratatui` already has `TestBackend`, `insta` already in dev-deps |

---

## Chunk 1: Architecture Refactor + Keyboard Navigation

### Task 1: Extract render method from run_tui closure

**Files:**
- Modify: `src/tui.rs:21-130`

The rendering logic is currently inlined in a `terminal.draw(|frame| { ... })` closure inside `run_tui`. Extract it to a method on `TuiApp` so it can be called from tests with `TestBackend`.

- [ ] **Step 1: Add `report_height` field and `render` method to TuiApp**

Add a `report_height: u16` field to track the visible height of the report pane (needed for PageUp/PageDown and scroll clamping). Add a `render` method that contains the current draw closure logic, plus computes `report_height`:

```rust
pub struct TuiApp {
    pub report_text: String,
    pub input: String,
    pub output_lines: Vec<String>,
    pub report_scroll: u16,
    pub should_quit: bool,
    /// Cursor position within `input` (byte offset). 0 = before first char.
    pub input_cursor: usize,
    /// Height of the report pane in the last render (for PageUp/PageDown).
    report_height: u16,
    /// Completion candidates currently shown.
    completions: Vec<String>,
    /// Index into `completions` for Tab cycling. None = no active completion.
    completion_idx: Option<usize>,
    /// Search highlight text.
    search_query: String,
}
```

```rust
impl TuiApp {
    pub fn render(&mut self, frame: &mut ratatui::Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(frame.area());

        // Track report pane inner height (subtract 2 for borders)
        self.report_height = chunks[0].height.saturating_sub(2);

        // --- Report pane ---
        let report_text = Text::raw(&self.report_text);
        let report_widget = Paragraph::new(report_text)
            .block(Block::default().borders(Borders::ALL).title(self.report_title()))
            .wrap(Wrap { trim: false })
            .scroll((self.report_scroll, 0));
        frame.render_widget(report_widget, chunks[0]);

        // --- Command pane ---
        let mut repl_lines: Vec<Line> = self.output_lines
            .iter()
            .map(|l| Line::raw(l.as_str()))
            .collect();

        // Prompt line with cursor
        let (before, cursor_ch, after) = self.split_input_at_cursor();
        repl_lines.push(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Green)),
            Span::raw(before),
            Span::styled(cursor_ch, Style::default().fg(Color::Black).bg(Color::Green)),
            Span::raw(after),
        ]));

        // Completion hint line (if active)
        if !self.completions.is_empty() {
            let hints: Vec<Span> = self.completions.iter().enumerate().map(|(i, c)| {
                let style = if self.completion_idx == Some(i) {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                Span::styled(format!(" {c} "), style)
            }).collect();
            repl_lines.push(Line::from(hints));
        }

        let repl_widget = Paragraph::new(Text::from(repl_lines))
            .block(Block::default().borders(Borders::ALL).title(" Commands "))
            .wrap(Wrap { trim: false });
        frame.render_widget(repl_widget, chunks[1]);
    }

    fn report_title(&self) -> String {
        if self.search_query.is_empty() {
            " Report ".to_string()
        } else {
            format!(" Report [searching: {}] ", self.search_query)
        }
    }

    fn split_input_at_cursor(&self) -> (String, String, String) {
        let cursor = self.input_cursor.min(self.input.len());
        let before = self.input[..cursor].to_string();
        if cursor < self.input.len() {
            let ch = self.input[cursor..cursor + 1].to_string();
            let after = self.input[cursor + 1..].to_string();
            (before, ch, after)
        } else {
            (before, " ".to_string(), String::new())
        }
    }
}
```

- [ ] **Step 2: Update `run_tui` to call `app.render(frame)` instead of inline closure**

```rust
pub fn run_tui(report_text: String) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new(report_text);

    loop {
        terminal.draw(|frame| app.render(frame))?;

        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if app.handle_key(key) {
                break;
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: compiles with no errors (warnings OK for now)

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "refactor: extract TuiApp::render method from run_tui closure"
```

### Task 2: Refactor input handling into handle_key method

**Files:**
- Modify: `src/tui.rs` (TuiApp impl block)

Move all key handling from the `run_tui` match into a `handle_key` method. This is the core of testability — tests can construct `KeyEvent` values and call `handle_key` directly.

- [ ] **Step 1: Define Action enum and handle_key method**

```rust
/// What the event loop should do after a key press.
pub enum Action {
    /// Continue the event loop.
    Continue,
    /// Break the event loop (quit).
    Quit,
}

impl TuiApp {
    /// Process a key event. Returns Quit to break the event loop.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> Action {
        // Ctrl+C always quits
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Action::Quit;
        }

        // Tab completion cycling
        if key.code == KeyCode::Tab && !self.completions.is_empty() {
            self.cycle_completion();
            return Action::Continue;
        }

        // If any non-tab key is pressed while completions are shown, dismiss them
        // (unless it's Enter which will execute the completed command)
        if key.code != KeyCode::Enter && key.code != KeyCode::Tab {
            self.completions.clear();
            self.completion_idx = None;
        }

        match key.code {
            KeyCode::Enter => self.submit_input(),
            KeyCode::Char(c) => {
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
            // Input cursor movement
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
            KeyCode::Home if key.modifiers.contains(KeyModifiers::NONE) && self.input.is_empty() => {
                self.report_scroll = 0;
                Action::Continue
            }
            KeyCode::End if key.modifiers.contains(KeyModifiers::NONE) && self.input.is_empty() => {
                self.scroll_to_bottom();
                Action::Continue
            }
            // Report scrolling (when input is empty, Up/Down scroll report)
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
            // Line editing shortcuts
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
                } else {
                    return Action::Quit;
                }
                Action::Continue
            }
            _ => Action::Continue,
        }
    }
}
```

- [ ] **Step 2: Add scroll helper methods**

```rust
impl TuiApp {
    /// Approximate total line count of the report (rough, doesn't account for wrapping).
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
}
```

- [ ] **Step 3: Update `run_tui` to use handle_key return value**

```rust
// In run_tui, replace the match block with:
if app.handle_key(key).should_quit() {
    break;
}
```

Add to Action:

```rust
impl Action {
    fn should_quit(&self) -> bool {
        matches!(self, Action::Quit)
    }
}
```

- [ ] **Step 4: Verify it compiles and the old `handle_command` still works**

Run: `cargo check`
Expected: compiles. The old `handle_command` is still called from `submit_input` (next task).

- [ ] **Step 5: Commit**

```bash
git add src/tui.rs
git commit -m "refactor: extract handle_key method with scroll bounds and input cursor"
```

### Task 3: Slash command system with tab completion

**Files:**
- Modify: `src/tui.rs` (TuiApp impl block)

- [ ] **Step 1: Define slash commands and submit_input**

```rust
/// Available slash commands.
const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/help", "Show available commands"),
    ("/quit", "Exit interactive mode"),
    ("/top", "Scroll to top of report"),
    ("/bottom", "Scroll to bottom of report"),
    ("/search", "Search report text — /search <query>"),
];

impl TuiApp {
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

        let response = self.execute_command(&cmd);
        if let Some(text) = response {
            for line in text.lines() {
                self.output_lines.push(line.to_string());
            }
        }

        if self.should_quit {
            Action::Quit
        } else {
            Action::Continue
        }
    }

    fn execute_command(&mut self, cmd: &str) -> Option<String> {
        let trimmed = cmd.trim();

        // Slash commands
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
                        // Scroll to first match
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

        // Legacy bare commands (backward compat)
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
}
```

- [ ] **Step 2: Add tab completion**

```rust
impl TuiApp {
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
```

- [ ] **Step 3: Verify it compiles**

Run: `cargo check`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "feat: add slash commands with tab completion and search"
```

---

## Chunk 2: Tests

### Task 4: Unit tests for state transitions

**Files:**
- Create: `tests/tui_test.rs`

- [ ] **Step 1: Write state transition tests**

```rust
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ceo::tui::TuiApp;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

#[test]
fn scroll_down_clamps_to_max() {
    let mut app = TuiApp::new("line1\nline2\nline3\n".to_string());
    app.report_height = 2; // can see 2 lines, so max scroll = 3-2 = 1
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.report_scroll, 1);
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.report_scroll, 1); // clamped
}

#[test]
fn page_up_from_middle() {
    let text = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let mut app = TuiApp::new(text);
    app.report_height = 10;
    app.report_scroll = 50;
    app.handle_key(key(KeyCode::PageUp));
    assert_eq!(app.report_scroll, 40);
}

#[test]
fn home_scrolls_to_top() {
    let mut app = TuiApp::new("a\nb\nc\n".to_string());
    app.report_scroll = 5;
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.report_scroll, 0);
}

#[test]
fn end_scrolls_to_bottom() {
    let text = (0..100).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let mut app = TuiApp::new(text);
    app.report_height = 10;
    app.handle_key(key(KeyCode::End));
    assert_eq!(app.report_scroll, 90); // 100 lines - 10 visible = 90
}

#[test]
fn input_cursor_movement() {
    let mut app = TuiApp::new(String::new());
    // Type "hello"
    for c in "hello".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    assert_eq!(app.input, "hello");
    assert_eq!(app.input_cursor, 5);

    // Left twice
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.input_cursor, 3);

    // Type 'X' at cursor
    app.handle_key(key(KeyCode::Char('X')));
    assert_eq!(app.input, "helXlo");
    assert_eq!(app.input_cursor, 4);
}

#[test]
fn ctrl_u_clears_input() {
    let mut app = TuiApp::new(String::new());
    for c in "/search".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(ctrl('u'));
    assert_eq!(app.input, "");
    assert_eq!(app.input_cursor, 0);
}

#[test]
fn slash_help_command() {
    let mut app = TuiApp::new(String::new());
    for c in "/help".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.output_lines.iter().any(|l| l.contains("/help")));
    assert!(app.output_lines.iter().any(|l| l.contains("/quit")));
}

#[test]
fn slash_quit_command() {
    let mut app = TuiApp::new(String::new());
    for c in "/quit".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    let action = app.handle_key(key(KeyCode::Enter));
    assert!(action.should_quit());
}

#[test]
fn tab_completion_cycles() {
    let mut app = TuiApp::new(String::new());
    // Type "/" — should show all commands
    app.handle_key(key(KeyCode::Char('/')));
    assert!(!app.completions.is_empty());

    // Type "s" — should narrow to /search
    app.handle_key(key(KeyCode::Char('s')));
    assert!(app.completions.iter().any(|c| c == "/search"));

    // Tab to complete
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.input, "/search");
}

#[test]
fn search_scrolls_to_match() {
    let text = (0..50).map(|i| {
        if i == 30 { "NEEDLE in haystack".to_string() }
        else { format!("line {i}") }
    }).collect::<Vec<_>>().join("\n");
    let mut app = TuiApp::new(text);
    app.report_height = 10;
    for c in "/search NEEDLE".chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.report_scroll, 30);
    assert_eq!(app.search_query, "NEEDLE");
}

#[test]
fn esc_clears_search_first_then_quits() {
    let mut app = TuiApp::new(String::new());
    app.search_query = "test".to_string();
    let action = app.handle_key(key(KeyCode::Esc));
    assert!(!action.should_quit());
    assert!(app.search_query.is_empty());

    // Second Esc quits
    let action = app.handle_key(key(KeyCode::Esc));
    assert!(action.should_quit());
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test tui_test`
Expected: all pass

- [ ] **Step 3: Commit**

```bash
git add tests/tui_test.rs
git commit -m "test: add unit tests for TUI state transitions and slash commands"
```

### Task 5: Rendering snapshot tests

**Files:**
- Modify: `tests/tui_test.rs` (append)
- Creates: `tests/snapshots/tui_test__*.snap` (auto-generated by insta)

- [ ] **Step 1: Add rendering tests with TestBackend**

```rust
use ratatui::{backend::TestBackend, Terminal};

#[test]
fn render_report_with_scroll_position() {
    let text = (0..20).map(|i| format!("Report line {i}")).collect::<Vec<_>>().join("\n");
    let mut app = TuiApp::new(text);
    app.report_scroll = 5;

    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();

    insta::assert_snapshot!("report_scrolled", terminal.backend().to_string());
}

#[test]
fn render_with_completions() {
    let mut app = TuiApp::new("# Report\nSome content.".to_string());
    app.input = "/s".to_string();
    app.input_cursor = 2;
    app.completions = vec!["/search".to_string()];
    app.completion_idx = Some(0);

    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();

    insta::assert_snapshot!("completions_visible", terminal.backend().to_string());
}

#[test]
fn render_with_search_active() {
    let text = "# Report\nFirst line\nNEEDLE here\nLast line".to_string();
    let mut app = TuiApp::new(text);
    app.search_query = "NEEDLE".to_string();

    let backend = TestBackend::new(60, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();

    let output = terminal.backend().to_string();
    assert!(output.contains("searching: NEEDLE"), "Report title should show search query");
    insta::assert_snapshot!("search_active", output);
}
```

- [ ] **Step 2: Run tests to generate snapshots**

Run: `cargo test --test tui_test`
Expected: fails with "new snapshot" messages

- [ ] **Step 3: Review and accept snapshots**

Run: `cargo insta accept`

- [ ] **Step 4: Run tests again to verify**

Run: `cargo test --test tui_test`
Expected: all pass

- [ ] **Step 5: Commit**

```bash
git add tests/tui_test.rs tests/snapshots/
git commit -m "test: add rendering snapshot tests for TUI"
```

### Task 6: Clean up old TuiApp::new and remove stubs

**Files:**
- Modify: `src/tui.rs` — update `TuiApp::new`, remove old `handle_command`

- [ ] **Step 1: Update TuiApp::new with all new fields**

```rust
impl TuiApp {
    pub fn new(report_text: String) -> Self {
        Self {
            report_text,
            input: String::new(),
            output_lines: vec!["Type /help for commands. Press Esc to quit.".to_string()],
            report_scroll: 0,
            should_quit: false,
            input_cursor: 0,
            report_height: 0,
            completions: Vec::new(),
            completion_idx: None,
            search_query: String::new(),
        }
    }
}
```

- [ ] **Step 2: Remove old `handle_command` method**

Delete the old `handle_command` method entirely (lines 40-59 in current code). All command handling now goes through `execute_command`.

- [ ] **Step 3: Run all tests**

Run: `cargo test`
Expected: all pass (except pre-existing pipeline failures)

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "chore: clean up TuiApp constructor and remove stub commands"
```

---

## Summary of keyboard bindings

| Key | Context | Effect |
|---|---|---|
| Up/Down | Always | Scroll report 1 line |
| PageUp/PageDown | Always | Scroll report by page |
| Home | Input empty | Scroll to top |
| End | Input empty | Scroll to bottom |
| Left/Right | Always | Move input cursor |
| Ctrl+A | Always | Cursor to start of input |
| Ctrl+E | Always | Cursor to end of input |
| Ctrl+U | Always | Clear input line |
| Tab | `/` prefix typed | Cycle through matching commands |
| Enter | Always | Execute command |
| Esc | Search active | Clear search |
| Esc | No search | Quit |
| Ctrl+C | Always | Quit |

## Slash commands

| Command | Effect |
|---|---|
| `/help` | Show all commands |
| `/quit` | Exit |
| `/top` | Scroll to top |
| `/bottom` | Scroll to bottom |
| `/search <text>` | Search and scroll to first match |
