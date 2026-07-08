//! Integration-style snapshot tests that drive `Reader<TestBackend>` through
//! synthetic key events and snapshot the rendered 80x24 screen with insta.

use super::Reader;
use crate::config::Config;
use crate::settings::{CfgDefaultKeymaps, Settings};
use crate::state::State;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;

fn test_reader() -> Reader<TestBackend> {
    let config = Config::with_settings(Settings::default(), CfgDefaultKeymaps::default()).unwrap();
    let mut reader = Reader::with_backend(config, TestBackend::new(80, 24), State::new_for_test())
        .expect("failed to construct test reader");

    let fixture_path = format!("{}/tests/fixtures/small.epub", env!("CARGO_MANIFEST_DIR"));
    reader
        .load_ebook(&fixture_path)
        .expect("failed to load fixture epub");

    // Loading a fresh book can leave a status message queued (and messages
    // may embed absolute paths), which would make snapshots nondeterministic.
    reader.state.borrow_mut().ui_state.clear_message();

    reader.draw().expect("failed to draw initial frame");
    reader
}

fn press(reader: &mut Reader<TestBackend>, code: KeyCode) {
    reader
        .handle_key_event(KeyEvent::new(code, KeyModifiers::NONE))
        .expect("key handling failed");
    reader.draw().expect("failed to draw frame after key press");
}

fn press_char(reader: &mut Reader<TestBackend>, c: char) {
    press(reader, KeyCode::Char(c));
}

fn type_str(reader: &mut Reader<TestBackend>, s: &str) {
    for c in s.chars() {
        press_char(reader, c);
    }
}

#[test]
fn initial_screen() {
    let reader = test_reader();
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn scroll_down() {
    let mut reader = test_reader();
    // Early chapters are mostly cover/ad pages; 40 lines gets past them into
    // actual paragraph text so the scroll is visible in the snapshot.
    for _ in 0..40 {
        press_char(&mut reader, 'j');
    }
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn help_window() {
    let mut reader = test_reader();
    press_char(&mut reader, '?');
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn toc_window() {
    let mut reader = test_reader();
    press_char(&mut reader, 't');
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn search_flow() {
    let mut reader = test_reader();
    press_char(&mut reader, '/');
    type_str(&mut reader, "Preface");
    // First Enter commits the query; the second jumps to the match and
    // closes the popup, leaving the highlighted hit visible in the page.
    press(&mut reader, KeyCode::Enter);
    press(&mut reader, KeyCode::Enter);
    // Drop the "Match 1/2" toast (it would expire after 3s in real usage)
    // so the highlighted hit itself isn't hidden underneath it.
    reader.state.borrow_mut().ui_state.clear_message();
    reader
        .draw()
        .expect("failed to draw after clearing message");
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn internal_link_preview() {
    let mut reader = test_reader();
    press_char(&mut reader, '/');
    type_str(&mut reader, "Preface");
    press(&mut reader, KeyCode::Enter);
    press(&mut reader, KeyCode::Enter);
    reader.state.borrow_mut().ui_state.clear_message();
    reader
        .draw()
        .expect("failed to draw after clearing message");

    press_char(&mut reader, 'u');
    press(&mut reader, KeyCode::Enter);
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn cursor_mode() {
    let mut reader = test_reader();
    press_char(&mut reader, 'v');
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn line_numbers() {
    let mut reader = test_reader();
    // There is no single "toggle line numbers" key; it lives in the
    // Settings window as the first item (index 0), toggled with Enter.
    press_char(&mut reader, 's');
    press(&mut reader, KeyCode::Enter);
    press_char(&mut reader, 'q');
    insta::assert_snapshot!(reader.terminal.backend());
}
