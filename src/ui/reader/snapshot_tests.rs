//! Integration-style snapshot tests that drive `Reader<TestBackend>` through
//! synthetic key events and snapshot the rendered 80x24 screen with insta.

use super::{READING_JUMP_MIN_THRESHOLD_ROWS, Reader, SettingItem};
use crate::config::Config;
use crate::models::ReadingState;
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
fn settings_window_sections() {
    let mut reader = test_reader();
    press_char(&mut reader, 's');
    reader.state.borrow_mut().ui_state.clear_message();
    reader.draw().expect("failed to draw settings window");
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn settings_window_scrolls_to_selection() {
    let mut reader = test_reader();
    press_char(&mut reader, 's');
    // Drive the selection down into the last section, which starts below the
    // initial fold; the list must scroll it into view.
    let pull_index = SettingItem::all()
        .iter()
        .position(|item| *item == SettingItem::KosyncPullNow)
        .expect("pull setting should exist");
    for _ in 0..pull_index {
        press_char(&mut reader, 'j');
    }
    reader.state.borrow_mut().ui_state.clear_message();
    reader.draw().expect("failed to draw settings window");
    let screen = format!("{}", reader.terminal.backend());
    assert!(
        screen.contains("Pull KOReader progress now"),
        "selecting the last setting should scroll it into view:\n{screen}"
    );
}

#[test]
fn typography_settings_reparse_the_full_book() {
    let mut reader = test_reader();
    let original_lines = reader.board.total_lines();
    press_char(&mut reader, 's');

    let paragraph_index = SettingItem::all()
        .iter()
        .position(|item| *item == SettingItem::ParagraphStyle)
        .unwrap();
    for _ in 0..paragraph_index {
        press_char(&mut reader, 'j');
    }
    press(&mut reader, KeyCode::Enter);
    assert_eq!(
        reader.state.borrow().config.settings.paragraph_style,
        crate::settings::ParagraphStyle::Compact
    );
    assert_eq!(
        reader.current_typography.paragraph_style,
        crate::settings::ParagraphStyle::Compact
    );
    assert!(reader.board.total_lines() <= original_lines);

    press_char(&mut reader, 'j');
    press(&mut reader, KeyCode::Enter);
    assert_eq!(
        reader.current_typography.line_spacing,
        crate::settings::LineSpacing::OneAndHalf
    );
    assert!(!reader.board.paragraph_starts().is_empty());
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
    reader.poll_library_cover();
    assert!(
        reader.library_cover_pending.is_none(),
        "cover loading should remain idle until explicitly enabled"
    );
    press_char(&mut reader, 'c');
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
    assert!(
        reader.library_cover_redraw_pending,
        "a newly decoded cover should schedule a follow-up frame"
    );
    // The details panel shows the full (wrapped) file path, which would
    // embed this machine's repo location; remap the entry (and its cached
    // cover) to a fixed path so the snapshot is deterministic.
    let real_path = reader
        .selected_library_path()
        .expect("a book should be selected");
    let fixed_path = "/books/small.epub".to_string();
    {
        let mut state = reader.state.borrow_mut();
        let index = state
            .ui_state
            .selected_list_index(state.ui_state.library_selected_index)
            .expect("selection resolves");
        state.ui_state.library_items[index].filepath = fixed_path.clone();
    }
    if let Some(cover) = reader.library_covers.remove(&real_path) {
        reader.library_covers.insert(fixed_path, cover);
    }
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

#[test]
fn legacy_position_without_source_offset_uses_restore_ladder() {
    let mut reader = test_reader();
    let configured_width = reader.state.borrow().reading_state.textwidth;
    let effective_width = reader.current_text_width.unwrap();
    assert_ne!(configured_width, effective_width);

    let legacy = ReadingState {
        content_index: 0,
        source_offset: None,
        textwidth: 40,
        row: usize::MAX,
        rel_pctg: Some(0.5),
        section: None,
    };

    assert_eq!(
        reader.restore_row(&legacy, 80),
        reader.board.row_for_fraction(0.5)
    );

    let same_width = ReadingState {
        textwidth: configured_width,
        row: 5,
        ..legacy.clone()
    };
    assert_eq!(reader.restore_row(&same_width, configured_width), 5);

    let captured = reader.position_state_for_row(5);
    assert_eq!(captured.textwidth, configured_width);
    assert_ne!(captured.textwidth, effective_width);

    {
        let mut state = reader.state.borrow_mut();
        state.reading_state.row = 10;
        state.jump_history = vec![same_width.clone()];
        state.jump_history_index = 1;
    }
    reader.jump_back();
    assert_eq!(reader.state.borrow().reading_state.row, 5);

    let bookmark = ReadingState {
        row: 6,
        ..same_width
    };
    {
        let mut state = reader.state.borrow_mut();
        state.ui_state.bookmarks = vec![("Legacy".to_string(), bookmark)];
        state.ui_state.bookmarks_selected_index = 0;
    }
    reader.jump_to_selected_bookmark().unwrap();
    assert_eq!(reader.state.borrow().reading_state.row, 6);

    let raw_fallback = ReadingState {
        rel_pctg: None,
        ..legacy
    };
    assert_eq!(
        reader.restore_row(&raw_fallback, 80),
        reader.board.total_lines() - 1
    );
}

#[test]
fn width_change_preserves_first_visible_sentence() {
    let mut settings = Settings {
        width: Some(50),
        ..Settings::default()
    };
    settings.seamless_between_chapters = true;
    let mut reader = test_reader_with_settings(settings);

    let (content_index, local_row) = reader
        .chapter_text_structures
        .iter()
        .enumerate()
        .find_map(|(content_index, chapter)| {
            chapter
                .text_lines
                .iter()
                .position(|line| line.starts_with("O’Reilly books may be purchased"))
                .map(|local_row| (content_index, local_row))
        })
        .expect("known fixture paragraph should be present");
    let row = reader.content_start_rows[content_index] + local_row;
    {
        let mut state = reader.state.borrow_mut();
        state.reading_state.row = row;
        state.reading_state.content_index = content_index;
    }
    let source_position = reader.source_position_for_row(row);

    press_char(&mut reader, '+');

    let restored_row = reader.state.borrow().reading_state.row;
    assert_eq!(
        reader.source_position_for_row(restored_row),
        source_position
    );
    let stored = reader
        .db_state
        .get_last_reading_state(reader.ebook.as_ref().unwrap().as_ref())
        .unwrap()
        .unwrap();
    assert_eq!(stored.content_index, content_index);
    assert_eq!(
        stored.source_offset,
        source_position.map(|(_, offset)| offset)
    );
    assert_eq!(stored.textwidth, 55);
    assert_eq!(stored.row, restored_row);
    assert!(stored.rel_pctg.is_some());
    let restored_local_row = restored_row - reader.content_start_rows[content_index];
    assert!(
        reader.chapter_text_structures[content_index].text_lines[restored_local_row]
            .starts_with("O’Reilly books may be purchased")
    );
    insta::assert_snapshot!(reader.terminal.backend());
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
        state.ui_state.pending_sync_progress = Some((0.42, "KOReader (kobo)".to_string(), 0));
        state
            .ui_state
            .open_window(crate::models::WindowType::ConfirmSyncProgress);
    }
    reader.draw().expect("failed to draw sync prompt");
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn kosync_xpointer_anchors_exact_chapter() {
    let reader = test_reader();
    let starts = reader.content_start_rows.clone();
    assert!(starts.len() >= 2, "fixture must have multiple chapters");
    let chapter = starts.len() - 1;
    let chapter_start = starts[chapter];

    // KOReader reports the very start of the last chapter via DocFragment[N].
    // The percentage is set to the chapter start so the plausibility guard
    // passes; the XPointer (not the percentage) drives the landing.
    let percentage = reader.board.content_fraction(chapter_start);
    let remote = crate::sync::RemoteProgress {
        document: "doc".into(),
        progress: format!("/body/DocFragment[{}]/body", chapter + 1),
        percentage,
        device: "KOReader".into(),
        device_id: String::new(),
        timestamp: 0,
    };

    let mut reader = reader;
    let row = reader.resolve_kosync_target_row(&remote);
    assert!(
        row.abs_diff(chapter_start) <= 1,
        "XPointer should anchor the chapter start (got {row}, want {chapter_start})"
    );
}

#[test]
fn kosync_falls_back_when_no_xpointer() {
    let mut reader = test_reader();
    // A bare percentage (what repy/older clients store) has no XPointer, so
    // resolution falls back to the content-percentage mapping.
    let remote = crate::sync::RemoteProgress {
        document: "doc".into(),
        progress: "0.50000000".into(),
        percentage: 0.5,
        device: "KOReader".into(),
        device_id: String::new(),
        timestamp: 0,
    };
    let row = reader.resolve_kosync_target_row(&remote);
    assert_eq!(row, reader.board.row_for_fraction(0.5));
}

fn sample_opds_feed() -> crate::opds::Feed {
    use crate::opds::{AcquisitionLink, Availability, NavigationEntry, Publication};
    let acquisition = |ext: &str, availability| AcquisitionLink {
        href: format!("https://example.test/book.{ext}"),
        media_type: None,
        relation: "http://opds-spec.org/acquisition".into(),
        availability,
        title: None,
        length: None,
    };
    crate::opds::Feed {
        title: "Sample Shelf".into(),
        navigation: vec![NavigationEntry {
            title: "Science Fiction".into(),
            href: "https://example.test/sf".into(),
            summary: None,
        }],
        publications: vec![
            Publication {
                title: "A Long Voyage".into(),
                authors: vec!["Ada Author".into(), "Bo Writer".into()],
                summary: Some("An expedition drifts far off course.".into()),
                cover: None,
                acquisitions: vec![
                    AcquisitionLink {
                        title: Some("EPUB (with images)".into()),
                        length: Some(2 * 1024 * 1024),
                        ..acquisition("epub", Availability::Readable)
                    },
                    acquisition("mobi", Availability::Readable),
                ],
            },
            Publication {
                title: "Untitled Draft".into(),
                authors: vec![],
                summary: None,
                cover: None,
                acquisitions: vec![acquisition("pdf", Availability::Restricted)],
            },
        ],
        pagination: crate::opds::Pagination {
            next: Some("https://example.test/page2".into()),
            ..Default::default()
        },
        search: None,
    }
}

#[test]
fn opds_feed_window() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        state.ui_state.opds_feed = Some(sample_opds_feed());
        state.ui_state.opds_selected_index = 1;
        state
            .ui_state
            .open_window(crate::models::WindowType::OpdsFeed);
    }
    reader.draw().expect("failed to draw OPDS feed window");
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn opds_feed_details_pane() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        state.ui_state.opds_feed = Some(sample_opds_feed());
        state.ui_state.opds_selected_index = 1;
        state
            .ui_state
            .open_window(crate::models::WindowType::OpdsFeed);
    }
    reader.draw().expect("failed to draw OPDS feed window");
    press_char(&mut reader, 'c');
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn opds_catalogs_window() {
    let mut reader = test_reader();
    reader
        .state
        .borrow_mut()
        .ui_state
        .open_window(crate::models::WindowType::OpdsCatalogs);
    reader.draw().expect("failed to draw OPDS catalogs window");
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn opds_catalogs_window_empty() {
    let mut settings = Settings::default();
    settings.opds_catalogs.clear();
    let mut reader = test_reader_with_settings(settings);
    reader
        .state
        .borrow_mut()
        .ui_state
        .open_window(crate::models::WindowType::OpdsCatalogs);
    reader
        .draw()
        .expect("failed to draw empty OPDS catalogs window");
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn opds_feed_counter_follows_selection() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        state.ui_state.opds_feed = Some(sample_opds_feed());
        state.ui_state.opds_selected_index = 0;
        state
            .ui_state
            .open_window(crate::models::WindowType::OpdsFeed);
    }
    reader.draw().expect("draw");
    let before = format!("{}", reader.terminal.backend());
    press_char(&mut reader, 'j');
    press_char(&mut reader, 'j');
    let after = format!("{}", reader.terminal.backend());
    assert!(before.contains("1/3"), "missing 1/3:\n{before}");
    assert!(after.contains("3/3"), "counter did not advance:\n{after}");
}

#[test]
fn opds_feed_counter_uses_opensearch_totals() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        let mut feed = sample_opds_feed();
        feed.title = "All Books".into();
        feed.navigation.clear();
        feed.pagination.total_results = Some(1234);
        feed.pagination.start_index = Some(26);
        state.ui_state.opds_feed = Some(feed);
        state.ui_state.opds_selected_index = 0;
        state
            .ui_state
            .open_window(crate::models::WindowType::OpdsFeed);
    }
    reader.draw().expect("draw");
    let screen = format!("{}", reader.terminal.backend());
    assert!(
        screen.contains("All Books · 26/1234"),
        "expected catalog-wide position:\n{screen}"
    );
    press_char(&mut reader, 'j');
    let screen = format!("{}", reader.terminal.backend());
    assert!(
        screen.contains("All Books · 27/1234"),
        "expected counter to advance:\n{screen}"
    );
}

#[test]
fn opds_feed_counter_shows_page_number_without_totals() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        state.ui_state.opds_feed = Some(sample_opds_feed());
        state.ui_state.opds_selected_index = 0;
        state.ui_state.opds_page = 3;
        state
            .ui_state
            .open_window(crate::models::WindowType::OpdsFeed);
    }
    reader.draw().expect("draw");
    let screen = format!("{}", reader.terminal.backend());
    assert!(
        screen.contains("Sample Shelf · 1/3 · page 3"),
        "expected page suffix:\n{screen}"
    );
}

#[test]
fn opds_feed_q_returns_to_library() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        state.ui_state.opds_feed = Some(sample_opds_feed());
        state
            .ui_state
            .open_window(crate::models::WindowType::OpdsFeed);
    }
    reader.draw().expect("draw");
    press_char(&mut reader, 'q');
    assert_eq!(
        reader.state.borrow().ui_state.active_window,
        crate::models::WindowType::Library
    );
}

#[test]
fn opds_download_progress_centered() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        state.ui_state.opds_feed = Some(sample_opds_feed());
        state.ui_state.opds_loading = true;
        state.ui_state.opds_downloading = true;
        state.ui_state.opds_downloaded_bytes = 512 * 1024;
        state.ui_state.opds_total_bytes = Some(1024 * 1024);
        state
            .ui_state
            .open_window(crate::models::WindowType::OpdsFeed);
    }
    reader.draw().expect("draw");
    insta::assert_snapshot!(reader.terminal.backend());
}

#[test]
fn opds_format_cycle_updates_tag_and_details() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        state.ui_state.opds_feed = Some(sample_opds_feed());
        state.ui_state.opds_selected_index = 1;
        state
            .ui_state
            .open_window(crate::models::WindowType::OpdsDetails);
    }
    reader.draw().expect("draw");
    let screen = format!("{}", reader.terminal.backend());
    assert!(screen.contains("[EPUB (with images) 1/2]"), "{screen}");
    assert!(
        screen.contains("▶ EPUB (with images) · 2.0 MiB"),
        "{screen}"
    );
    press_char(&mut reader, 'f');
    let screen = format!("{}", reader.terminal.backend());
    assert!(screen.contains("[MOBI 2/2]"), "{screen}");
    assert!(screen.contains("▶ MOBI"), "{screen}");
}

#[test]
fn opds_long_variant_label_keeps_counter() {
    let mut reader = test_reader();
    {
        let mut state = reader.state.borrow_mut();
        let mut feed = sample_opds_feed();
        feed.publications[0].acquisitions[0].title =
            Some("EPUB3 (E-readers incl. Send-to-Kindle)".into());
        state.ui_state.opds_feed = Some(feed);
        state.ui_state.opds_selected_index = 1;
        state
            .ui_state
            .open_window(crate::models::WindowType::OpdsFeed);
    }
    reader.draw().expect("draw");
    let screen = format!("{}", reader.terminal.backend());
    // On the 58-column popup the long Gutenberg label is shortened, but
    // never below the row's spare room, and the cycle counter survives.
    assert!(screen.contains("… 1/2]"), "counter lost:\n{screen}");
    assert!(
        screen.contains("[EPUB3 (E-readers incl."),
        "label over-truncated:\n{screen}"
    );
}

#[test]
fn status_message_not_covered_by_inline_image() {
    let mut settings = Settings::default();
    settings.inline_images = crate::settings::InlineImages::Shown;
    let mut reader = test_reader_with_settings(settings);
    reader.graphics = crate::ui::graphics::Graphics::halfblocks_for_test();
    reader.poll_inline_images();
    while reader.inline_images_pending {
        reader.poll_inline_images();
    }
    // A toast (e.g. the calibredb outcome) must stay readable even though
    // the cover image block overlaps its area: images pause while visible.
    reader
        .state
        .borrow_mut()
        .ui_state
        .set_message("Added to Calibre library".into(), super::MessageType::Info);
    reader.draw().expect("draw");
    let screen = format!("{}", reader.terminal.backend());
    assert!(
        screen.contains("Added to Calibre library"),
        "message hidden by inline image:\n{screen}"
    );
}

#[test]
fn warning_toast_persists_until_key_dismisses_it() {
    let mut reader = test_reader();
    let start_row = reader.state.borrow().reading_state.row;
    reader.state.borrow_mut().ui_state.set_message(
        "calibredb: something went wrong".into(),
        super::MessageType::Warning,
    );
    // Warnings never auto-expire...
    assert!(!reader.state.borrow().ui_state.message_expired());
    reader.draw().expect("draw");
    let screen = format!("{}", reader.terminal.backend());
    assert!(screen.contains("press any key to dismiss"), "{screen}");
    // Capped-width, centered toast box.
    insta::assert_snapshot!("warning_toast_centered", reader.terminal.backend());
    // ...and the dismissing key is consumed instead of scrolling the reader.
    press_char(&mut reader, 'j');
    assert!(reader.state.borrow().ui_state.message.is_none());
    assert_eq!(reader.state.borrow().reading_state.row, start_row);
    // The next key acts normally again.
    press_char(&mut reader, 'j');
    assert_eq!(reader.state.borrow().reading_state.row, start_row + 1);
}

#[test]
fn move_to_calibre_guards_missing_file() {
    let mut reader = test_reader();
    reader
        .db_state
        .upsert_library_file(
            "/nonexistent/gone.epub",
            1,
            Some("Vanished Book"),
            Some("No One"),
        )
        .unwrap();
    press_char(&mut reader, 'r');
    let index = {
        let state = reader.state.borrow();
        state
            .ui_state
            .library_items
            .iter()
            .position(|item| item.filepath == "/nonexistent/gone.epub")
            .expect("missing entry should be listed")
    };
    reader.state.borrow_mut().ui_state.library_selected_index = index;
    press_char(&mut reader, 'm');
    let state = reader.state.borrow();
    let message = state.ui_state.message.clone().unwrap_or_default();
    assert!(message.contains("missing"), "unexpected message: {message}");
    assert!(reader.calibre_import_rx.is_none());
}
