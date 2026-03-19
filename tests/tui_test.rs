use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};

use ceo::tui_app::TuiApp;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn type_str(app: &mut TuiApp, s: &str) {
    for c in s.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
}

// =========================================================================
// Scroll tests
// =========================================================================

#[test]
fn scroll_down_clamps_to_max() {
    let mut app = TuiApp::new("line1\nline2\nline3".to_string());
    app.report_height = 2;
    app.handle_key(key(KeyCode::PageDown));
    assert_eq!(app.report_scroll, 1); // 3 lines - 2 visible = max 1
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
    assert_eq!(app.report_scroll, 90);
}

#[test]
fn arrow_up_down_scroll_one_line() {
    let text = (0..20).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let mut app = TuiApp::new(text);
    app.report_height = 10;
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.report_scroll, 3);
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.report_scroll, 2);
}

#[test]
fn scroll_up_at_zero_stays_zero() {
    let mut app = TuiApp::new("one line".to_string());
    app.handle_key(key(KeyCode::Up));
    assert_eq!(app.report_scroll, 0);
}

// =========================================================================
// Input cursor tests
// =========================================================================

#[test]
fn input_cursor_movement() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "hello");
    assert_eq!(app.input, "hello");
    assert_eq!(app.input_cursor, 5);

    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.input_cursor, 3);

    // Insert at cursor
    app.handle_key(key(KeyCode::Char('X')));
    assert_eq!(app.input, "helXlo");
    assert_eq!(app.input_cursor, 4);
}

#[test]
fn backspace_at_cursor() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "abcd");
    app.handle_key(key(KeyCode::Left)); // cursor at 3
    app.handle_key(key(KeyCode::Backspace)); // delete 'c'
    assert_eq!(app.input, "abd");
    assert_eq!(app.input_cursor, 2);
}

#[test]
fn ctrl_u_clears_input() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "/search");
    app.handle_key(ctrl('u'));
    assert_eq!(app.input, "");
    assert_eq!(app.input_cursor, 0);
}

#[test]
fn ctrl_a_and_ctrl_e() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "hello");
    app.handle_key(ctrl('a'));
    assert_eq!(app.input_cursor, 0);
    app.handle_key(ctrl('e'));
    assert_eq!(app.input_cursor, 5);
}

#[test]
fn left_at_zero_stays() {
    let mut app = TuiApp::new(String::new());
    app.handle_key(key(KeyCode::Left));
    assert_eq!(app.input_cursor, 0);
}

#[test]
fn right_at_end_stays() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "hi");
    app.handle_key(key(KeyCode::Right));
    assert_eq!(app.input_cursor, 2); // already at end
}

// =========================================================================
// Command tests
// =========================================================================

#[test]
fn slash_help_outputs_commands() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "/help");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.output_lines.iter().any(|l: &String| l.contains("/help")));
    assert!(app.output_lines.iter().any(|l: &String| l.contains("/quit")));
    assert!(app.output_lines.iter().any(|l: &String| l.contains("/search")));
}

#[test]
fn slash_quit_returns_quit() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "/quit");
    let action = app.handle_key(key(KeyCode::Enter));
    assert!(action.should_quit());
}

#[test]
fn bare_quit_still_works() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "quit");
    let action = app.handle_key(key(KeyCode::Enter));
    assert!(action.should_quit());
}

#[test]
fn slash_top_scrolls_to_zero() {
    let mut app = TuiApp::new("a\nb\nc".to_string());
    app.report_scroll = 5;
    type_str(&mut app, "/top");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.report_scroll, 0);
}

#[test]
fn slash_bottom_scrolls_to_end() {
    let text = (0..50).map(|i| format!("line {i}")).collect::<Vec<_>>().join("\n");
    let mut app = TuiApp::new(text);
    app.report_height = 10;
    type_str(&mut app, "/bottom");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.report_scroll, 40);
}

#[test]
fn search_scrolls_to_match() {
    let text = (0..50).map(|i| {
        if i == 30 { "NEEDLE in haystack".to_string() }
        else { format!("line {i}") }
    }).collect::<Vec<_>>().join("\n");
    let mut app = TuiApp::new(text);
    app.report_height = 10;
    type_str(&mut app, "/search NEEDLE");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.report_scroll, 30);
    assert_eq!(app.search_query, "NEEDLE");
}

#[test]
fn search_no_match() {
    let mut app = TuiApp::new("hello world".to_string());
    type_str(&mut app, "/search MISSING");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.output_lines.iter().any(|l: &String| l.contains("No match")));
}

#[test]
fn search_clear() {
    let mut app = TuiApp::new(String::new());
    app.search_query = "old".to_string();
    type_str(&mut app, "/search");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.search_query.is_empty());
}

#[test]
fn unknown_command() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "/bogus");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.output_lines.iter().any(|l: &String| l.contains("Unknown command")));
}

#[test]
fn esc_clears_search_first_then_quits() {
    let mut app = TuiApp::new(String::new());
    app.search_query = "test".to_string();
    let action = app.handle_key(key(KeyCode::Esc));
    assert!(!action.should_quit());
    assert!(app.search_query.is_empty());

    let action = app.handle_key(key(KeyCode::Esc));
    assert!(action.should_quit());
}

#[test]
fn ctrl_c_quits() {
    let mut app = TuiApp::new(String::new());
    let action = app.handle_key(ctrl('c'));
    assert!(action.should_quit());
}

// =========================================================================
// Tab completion tests
// =========================================================================

#[test]
fn typing_slash_shows_completions() {
    let mut app = TuiApp::new(String::new());
    app.handle_key(key(KeyCode::Char('/')));
    assert!(!app.completions.is_empty());
}

#[test]
fn typing_slash_s_narrows_completions() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "/s");
    assert!(app.completions.iter().any(|c| c == "/search"));
    // /help, /quit, /top, /bottom should not be in completions
    assert!(!app.completions.iter().any(|c| c == "/help"));
}

#[test]
fn tab_completes_command() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "/se");
    assert!(app.completions.contains(&"/search".to_string()));
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.input, "/search");
    assert_eq!(app.input_cursor, 7);
}

#[test]
fn tab_cycles_multiple_completions() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "/"); // all commands match
    let count = app.completions.len();
    assert!(count > 1);

    app.handle_key(key(KeyCode::Tab));
    let first = app.input.clone();
    app.handle_key(key(KeyCode::Tab));
    let second = app.input.clone();
    assert_ne!(first, second);
}

#[test]
fn non_slash_input_no_completions() {
    let mut app = TuiApp::new(String::new());
    type_str(&mut app, "hello");
    assert!(app.completions.is_empty());
}

#[test]
fn empty_input_no_submit() {
    let mut app = TuiApp::new(String::new());
    let initial_len = app.output_lines.len();
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.output_lines.len(), initial_len); // nothing added
}

// =========================================================================
// Rendering snapshot tests (TestBackend + insta)
// =========================================================================

#[test]
fn render_initial_state() {
    let mut app = TuiApp::new("# Report\nLine 1\nLine 2\nLine 3".to_string());
    let backend = TestBackend::new(50, 15);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    insta::assert_snapshot!("tui_initial", terminal.backend().to_string());
}

#[test]
fn render_with_scroll() {
    let text = (0..30).map(|i| format!("Report line {i}")).collect::<Vec<_>>().join("\n");
    let mut app = TuiApp::new(text);
    app.report_scroll = 10;
    let backend = TestBackend::new(50, 15);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    insta::assert_snapshot!("tui_scrolled", terminal.backend().to_string());
}

#[test]
fn render_with_completions() {
    let mut app = TuiApp::new("# Report".to_string());
    app.input = "/s".to_string();
    app.input_cursor = 2;
    app.completions = vec!["/search".to_string()];
    app.completion_idx = Some(0);
    let backend = TestBackend::new(50, 15);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    insta::assert_snapshot!("tui_completions", terminal.backend().to_string());
}

#[test]
fn render_with_search_title() {
    let mut app = TuiApp::new("# Report\nNEEDLE here".to_string());
    app.search_query = "NEEDLE".to_string();
    let backend = TestBackend::new(50, 15);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| app.render(frame)).unwrap();
    let output = terminal.backend().to_string();
    assert!(output.contains("searching: NEEDLE"));
    insta::assert_snapshot!("tui_search", output);
}
