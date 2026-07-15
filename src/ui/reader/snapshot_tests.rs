//! Integration-style snapshot tests that drive `Reader<TestBackend>` through
//! synthetic key events and snapshot the rendered 80x24 screen with insta.

use super::{READING_JUMP_MIN_THRESHOLD_ROWS, Reader};
use crate::config::Config;
use crate::settings::{CfgDefaultKeymaps, Settings};
use crate::state::State;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::backend::TestBackend;

fn test_reader() -> Reader<TestBackend> {
    test_reader_with_settings(Settings::default())
}

fn test_reader_with_settings(settings: Settings) -> Reader<TestBackend> {
    let config = Config::with_settings(settings, CfgDefaultKeymaps::default()).unwrap();
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
fn inline_image_rendering() {
    let mut settings = Settings::default();
    settings.inline_images = crate::settings::InlineImages::Shown;
    let mut reader = test_reader_with_settings(settings);
    reader.graphics = crate::ui::graphics::Graphics::halfblocks_for_test();
    // The cover image block starts at row 0, so it is fully visible on the
    // initial page (the policy renders fully-visible blocks only).
    // The run loop drives decoding; step it manually here.
    reader.poll_inline_images();
    while reader.inline_images_pending {
        reader.poll_inline_images();
    }
    assert!(
        reader.inline_image_protocols.values().any(|p| p.is_some()),
        "the visible inline image should have decoded"
    );
    reader.draw().expect("failed to draw inline image");
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn images_window() {
    let mut reader = test_reader();
    // The first page shows the cover image placeholder.
    press_char(&mut reader, 'o');
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn image_view_window() {
    let mut reader = test_reader();
    // A fixed halfblocks picker stands in for terminal capability detection.
    reader.graphics = crate::ui::graphics::Graphics::halfblocks_for_test();
    press_char(&mut reader, 'o');
    press(&mut reader, KeyCode::Enter);
    assert!(reader.image_view.is_some(), "image viewer should be open");
    insta::assert_snapshot!("image_view_window", reader.terminal.backend());

    // Esc returns to the images list and drops the render state.
    press(&mut reader, KeyCode::Esc);
    assert!(reader.image_view.is_none());
    insta::assert_snapshot!("image_view_window_closed", reader.terminal.backend());
}

#[test]
fn cursor_mode() {
    let mut reader = test_reader();
    press_char(&mut reader, 'v');
    insta::assert_snapshot!(reader.terminal.backend());
}

/// Mimic the run loop: handle a key event, then record reading activity
/// with the row observed before the event.
fn press_recorded(reader: &mut Reader<TestBackend>, code: KeyCode) {
    let previous_row = reader.state.borrow().reading_state.row;
    reader
        .handle_key_event(KeyEvent::new(code, KeyModifiers::NONE))
        .expect("key handling failed");
    reader
        .record_reading_activity(previous_row)
        .expect("recording reading activity failed");
}

#[test]
fn reading_stats_dedup_and_jump_detection() {
    let mut reader = test_reader();

    // Linear scrolling counts each row (and its words) exactly once.
    for _ in 0..3 {
        press_recorded(&mut reader, KeyCode::Char('j'));
    }
    let expected_words = reader.count_words_in_range(0, 3);
    let session = reader.reading_session.as_ref().expect("session active");
    assert_eq!(session.rows, 3);
    assert_eq!(session.words, expected_words);

    // Re-reading the same span (up and back down) must not double-count.
    for _ in 0..3 {
        press_recorded(&mut reader, KeyCode::Char('k'));
    }
    for _ in 0..3 {
        press_recorded(&mut reader, KeyCode::Char('j'));
    }
    let session = reader.reading_session.as_ref().expect("session active");
    assert_eq!(session.rows, 3);
    assert_eq!(session.words, expected_words);

    // A large jump (G = end of book) advances the high-water mark without
    // counting the skipped span as read.
    let jump_threshold = reader.page_size().max(READING_JUMP_MIN_THRESHOLD_ROWS);
    assert!(
        reader.board.total_lines() > jump_threshold + 3,
        "fixture must be large enough to trigger jump detection"
    );
    press_recorded(&mut reader, KeyCode::Char('G'));
    let current_row = reader.state.borrow().reading_state.row;
    let session = reader.reading_session.as_ref().expect("session active");
    assert_eq!(session.rows, 3);
    assert_eq!(session.words, expected_words);
    assert_eq!(session.max_counted_row, current_row);
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

/// The last-read timestamp embeds the current time, and the history entry's
/// filepath embeds the repo checkout path; redact both.
fn library_snapshot_filters() -> Vec<(&'static str, &'static str)> {
    vec![
        (r"\d{2}:\d{2}(AM|PM) \w{3} \d{2}", "[time]"),
        (r"\([^)]*small\.epub\)?", "([path]/small.epub)"),
    ]
}

#[test]
fn library_window() {
    let mut reader = test_reader();
    // A book the scanner found on disk but that was never opened.
    reader
        .db_state
        .upsert_library_file(
            "/scanned/example.epub",
            1,
            Some("Scanned Book"),
            Some("Some Author"),
        )
        .unwrap();
    press_char(&mut reader, 'r');
    insta::with_settings!({filters => library_snapshot_filters()}, {
        insta::assert_snapshot!(reader.terminal.backend());
    });
}

#[test]
fn library_cover_panel() {
    let mut reader = test_reader();
    reader.graphics = crate::ui::graphics::Graphics::halfblocks_for_test();
    press_char(&mut reader, 'r');
    // The run loop drives the debounced cover load; step it manually here.
    reader.poll_library_cover();
    std::thread::sleep(super::LIBRARY_COVER_DEBOUNCE);
    reader.poll_library_cover();
    assert!(
        reader
            .selected_library_path()
            .is_some_and(|p| matches!(reader.library_covers.get(&p), Some(Some(_)))),
        "cover protocol should be cached for the selected book"
    );
    reader.draw().expect("failed to draw with cover panel");
    insta::with_settings!({filters => library_snapshot_filters()}, {
        insta::assert_snapshot!(reader.terminal.backend());
    });
}

#[test]
fn library_window_sorted_by_title() {
    let mut reader = test_reader();
    reader
        .db_state
        .upsert_library_file(
            "/scanned/example.epub",
            1,
            Some("A Scanned Book"),
            Some("Some Author"),
        )
        .unwrap();
    press_char(&mut reader, 'r');
    // 's' cycles recent → title; the scanned book sorts first.
    press_char(&mut reader, 's');
    insta::with_settings!({filters => library_snapshot_filters()}, {
        insta::assert_snapshot!(reader.terminal.backend());
    });
}

/// Switching books must save the outgoing book's position: reopening it
/// restores the row reached right before the switch, not the state from the
/// last quit.
#[test]
fn position_persists_across_book_switch() {
    let mut reader = test_reader();
    for _ in 0..7 {
        press_char(&mut reader, 'j');
    }
    assert_eq!(reader.state.borrow().reading_state.row, 7);

    let other = format!(
        "{}/tests/fixtures/meditations.epub",
        env!("CARGO_MANIFEST_DIR")
    );
    reader
        .load_ebook(&other)
        .expect("failed to load second book");
    assert_eq!(reader.state.borrow().reading_state.row, 0);

    let first = format!("{}/tests/fixtures/small.epub", env!("CARGO_MANIFEST_DIR"));
    reader
        .load_ebook(&first)
        .expect("failed to reload first book");
    assert_eq!(reader.state.borrow().reading_state.row, 7);
}

/// Paging must never start the window inside a reserved image block (the
/// image would be hidden and the page mostly blank): forward moves snap to
/// the block's first row, backward moves bottom-align the block.
#[test]
fn page_moves_snap_around_image_blocks() {
    let mut settings = Settings::default();
    settings.inline_images = crate::settings::InlineImages::Shown;
    let reader = test_reader_with_settings(settings);

    // small.epub's cover block starts at row 0.
    let rows = reader
        .board
        .image_block_rows(0)
        .expect("cover image block reserved");
    assert!(rows > 2, "cover block should span several rows");
    let page = reader.page_size();
    assert!(rows <= page, "block must fit a page for the snap to apply");

    // A forward move from above the block that lands inside it snaps to the
    // block start; a backward one bottom-aligns the block.
    // (small.epub's block starts at row 0, so "above" is impossible; the
    // freeze-avoidance branch below is the reachable forward case here.)
    assert_eq!(
        reader.snap_page_start_for_image_block(rows - 1, page, rows + 5, false),
        Some(rows.saturating_sub(page))
    );
    // A forward move already aligned on the block (current_start == block
    // start) must continue past it, not re-snap in place — re-snapping froze
    // paging when a chapter's clamped last page began inside the block.
    assert_eq!(
        reader.snap_page_start_for_image_block(1, page, 0, true),
        Some(rows)
    );
    // Starts outside the block (or on the placeholder row) need no snap.
    assert_eq!(
        reader.snap_page_start_for_image_block(0, page, 0, true),
        None
    );
    assert_eq!(
        reader.snap_page_start_for_image_block(rows, page, 0, true),
        None
    );
}

/// The shown-when-fully-visible policy: once the block scrolls partially
/// off-screen the image must disappear, leaving only the reserved rows.
#[test]
fn inline_image_hidden_when_partially_visible() {
    let mut settings = Settings::default();
    settings.inline_images = crate::settings::InlineImages::Shown;
    let mut reader = test_reader_with_settings(settings);
    reader.graphics = crate::ui::graphics::Graphics::halfblocks_for_test();
    reader.poll_inline_images();
    while reader.inline_images_pending {
        reader.poll_inline_images();
    }
    for _ in 0..4 {
        press_char(&mut reader, 'j');
    }
    assert!(
        reader.visible_inline_image_blocks().is_empty(),
        "partially visible block must not be scheduled"
    );
    reader.draw().expect("failed to draw after scroll");
    assert!(
        !format!("{}", reader.terminal.backend()).contains('▄'),
        "partially visible block must not render"
    );
}

#[test]
fn confirm_sync_progress_prompt() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        state.ui_state.pending_sync_progress = Some((0.42, "KOReader (kobo)".to_string()));
        state
            .ui_state
            .open_window(crate::models::WindowType::ConfirmSyncProgress);
    }
    reader.draw().expect("failed to draw sync prompt");
    insta::assert_snapshot!(reader.terminal.backend());
}
