# Visual Mode Enhancement: Cross-Page Character-Level Selection

## Context

The current visual mode (`v` key) is line-granularity only. It tracks `selection_start: Option<usize>` as a line index and highlights entire lines via `Modifier::REVERSED`. There is no column tracking — `h`/`l` keys call `move_cursor(Left/Right)` which are no-ops (fall through to `_ => {}` in `move_cursor`). There is also a bug: entering visual mode initializes `selection_start` to `0` instead of the current reading row.

This plan upgrades visual mode to support:
- Character-level selection (not just whole lines)
- Cross-page selection (selection persists while scrolling)
- `v` to start, `hjkl` to move cursor, `y` to yank, `d` to dictionary lookup
- Visual feedback: reversed highlighting on exactly the selected characters

---

## Architecture Overview (Read-Only Reference)

Key files and their roles:

| File | Role |
|---|---|
| `src/models.rs` | Data models — `UiState`, `TextStructure`, `ReadingState`, `CharPos` |
| `src/ui/reader.rs` | Main app state, event handling, `move_cursor`, `yank_selection` |
| `src/ui/board.rs` | Rendering — `render_content()` builds `Vec<Line>` for ratatui |

Key structures:
- `ReadingState.row: usize` — the absolute line index at top of viewport
- `UiState.selection_start: Option<usize>` — currently line-only anchor
- `TextStructure.text_lines: Vec<String>` — all wrapped lines concatenated across chapters
- `Board.get_selected_text(start, end)` — joins lines in range with `\n`
- `CharPos { row: u16, col: u16 }` — already exists in `models.rs` but unused by visual mode

---

## Step 1: Replace Selection State with Character-Level Anchors

### File: `src/ui/reader.rs` (UiState struct, ~line 118)

**Replace** the single field:
```rust
pub selection_start: Option<usize>,
```

**With** two fields:
```rust
pub visual_anchor: Option<(usize, usize)>,  // (row, col) — where 'v' was pressed
pub visual_cursor: Option<(usize, usize)>,  // (row, col) — current cursor position
```

Both are `None` when not in visual mode. `visual_anchor` is set once on entering visual mode and never changes until exit. `visual_cursor` moves with `hjkl`.

### Update `UiState::new()` (~line 152)

Initialize both to `None`.

### Update `open_window()` (~line 212)

**For `WindowType::Visual` (line 239-241):** This is buggy — it uses `self.selection_start.unwrap_or(0)` which ignores the current reading position. The caller must pass in the current row. Change the approach:

Do NOT set the visual anchor inside `open_window`. Instead, set it in the `'v'` key handler in `handle_reader_keys` (~line 737):

```rust
KeyCode::Char('v') => {
    let mut state = self.state.borrow_mut();
    let current_row = state.reading_state.row;
    // Place cursor at first character of current viewport top line
    state.ui_state.visual_anchor = Some((current_row, 0));
    state.ui_state.visual_cursor = Some((current_row, 0));
    state.ui_state.open_window(WindowType::Visual);
}
```

In `open_window`, the `WindowType::Visual` arm becomes a no-op (or just sets `active_window`):
```rust
WindowType::Visual => {
    // anchor/cursor already set by caller
}
```

**For `WindowType::Reader` (line 215-225):** Clear both fields:
```rust
self.visual_anchor = None;
self.visual_cursor = None;
```

Remove the old `self.selection_start = None;` line.

---

## Step 2: Implement Character-Level Cursor Movement

### File: `src/ui/reader.rs`

**Add a new method** `move_visual_cursor` on `Reader` (place near `move_cursor`, ~line 1567):

```rust
fn move_visual_cursor(&mut self, direction: AppDirection) {
    let mut state = self.state.borrow_mut();
    let total_lines = self.board.total_lines();

    if let Some((row, col)) = state.ui_state.visual_cursor {
        let text_lines = &self.board.text_structure_ref().text_lines;
        let (new_row, new_col) = match direction {
            AppDirection::Left => {
                if col > 0 {
                    (row, col - 1)
                } else if row > 0 {
                    // Wrap to end of previous line
                    let prev_len = text_lines[row - 1].chars().count();
                    (row - 1, prev_len.saturating_sub(1))
                } else {
                    (row, col)
                }
            }
            AppDirection::Right => {
                let line_len = text_lines[row].chars().count();
                if col + 1 < line_len {
                    (row, col + 1)
                } else if row + 1 < total_lines {
                    // Wrap to start of next line
                    (row + 1, 0)
                } else {
                    (row, col)
                }
            }
            AppDirection::Up => {
                if row > 0 {
                    let prev_len = text_lines[row - 1].chars().count();
                    (row - 1, col.min(prev_len.saturating_sub(1)))
                } else {
                    (row, col)
                }
            }
            AppDirection::Down => {
                if row + 1 < total_lines {
                    let next_len = text_lines[row + 1].chars().count();
                    (row + 1, col.min(next_len.saturating_sub(1)))
                } else {
                    (row, col)
                }
            }
            _ => (row, col),
        };
        state.ui_state.visual_cursor = Some((new_row, new_col));

        // Auto-scroll: if cursor goes off screen, adjust reading_state.row
        let page_size = self.page_size();
        let viewport_start = state.reading_state.row;
        let viewport_end = viewport_start + page_size;
        if new_row < viewport_start {
            state.reading_state.row = new_row;
        } else if new_row >= viewport_end {
            state.reading_state.row = new_row - page_size + 1;
        }
    }
}
```

Note: `text_structure_ref()` doesn't exist yet — you need to add a getter on `Board` that returns `&TextStructure` (see Step 5). Alternatively, access it through the existing `self.board.text_structure` if the field is `pub`.

**Important:** All character positions are in **chars** (Unicode grapheme-aware), not bytes. Use `.chars().count()` for line length, and use char indexing for slicing (see Step 4).

### Update `handle_visual_mode_keys` (~line 888)

Replace the `move_cursor` calls with `move_visual_cursor`:

```rust
fn handle_visual_mode_keys(&mut self, key: KeyEvent, repeat_count: u32) -> eyre::Result<()> {
    match key.code {
        KeyCode::Esc => {
            let mut state = self.state.borrow_mut();
            state.ui_state.open_window(WindowType::Reader);
        }
        KeyCode::Char('y') => {
            self.yank_selection()?;
        }
        KeyCode::Char('d') => {
            self.dictionary_lookup()?;
        }
        // Navigation — character-level
        KeyCode::Char('j') | KeyCode::Down => {
            for _ in 0..repeat_count {
                self.move_visual_cursor(AppDirection::Down);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            for _ in 0..repeat_count {
                self.move_visual_cursor(AppDirection::Up);
            }
        }
        KeyCode::Char('h') | KeyCode::Left => {
            for _ in 0..repeat_count {
                self.move_visual_cursor(AppDirection::Left);
            }
        }
        KeyCode::Char('l') | KeyCode::Right => {
            for _ in 0..repeat_count {
                self.move_visual_cursor(AppDirection::Right);
            }
        }
        // Word motions
        KeyCode::Char('w') => {
            for _ in 0..repeat_count {
                self.move_visual_cursor_word_forward();
            }
        }
        KeyCode::Char('b') => {
            for _ in 0..repeat_count {
                self.move_visual_cursor_word_backward();
            }
        }
        KeyCode::Char('e') => {
            for _ in 0..repeat_count {
                self.move_visual_cursor_word_end();
            }
        }
        // Line motions
        KeyCode::Char('0') if repeat_count == 1 => {
            // Move to beginning of line (only if not part of a count)
            let mut state = self.state.borrow_mut();
            if let Some((row, _)) = state.ui_state.visual_cursor {
                state.ui_state.visual_cursor = Some((row, 0));
            }
        }
        KeyCode::Char('$') => {
            let mut state = self.state.borrow_mut();
            if let Some((row, _)) = state.ui_state.visual_cursor {
                let line_len = self.board.line_char_count(row);
                state.ui_state.visual_cursor = Some((row, line_len.saturating_sub(1)));
            }
        }
        _ => {}
    }
    Ok(())
}
```

**Word motion helpers** (`move_visual_cursor_word_forward`, `_backward`, `_end`): Implement standard vim `w`, `b`, `e` motions. These scan characters in `text_lines` looking for word boundaries (transitions between whitespace/punctuation/alphanumeric). They are optional for the first version — `hjkl` alone is functional. If you skip them, remove the `w`/`b`/`e` arms from the match.

---

## Step 3: Update Rendering for Character-Level Highlighting

### File: `src/ui/board.rs` — `render_content()` (~line 91-139)

The current code checks `selection_start` and applies `Modifier::REVERSED` to entire lines. Replace this with character-level span splitting.

**Replace the selection block** (lines 118-124):
```rust
let mut style = Style::default();
if let Some(selection_start) = selection_start {
    let selection_end = state.reading_state.row;
    if (line_num >= selection_start && line_num <= selection_end) || ... {
        style = style.add_modifier(Modifier::REVERSED);
    }
}
```

**With logic that reads `visual_anchor` and `visual_cursor`:**

```rust
// Determine selection range (anchor..cursor, normalized to start <= end)
let selection_range: Option<((usize, usize), (usize, usize))> = {
    match (state.ui_state.visual_anchor, state.ui_state.visual_cursor) {
        (Some(anchor), Some(cursor)) => {
            let (start, end) = if anchor <= cursor {
                (anchor, cursor)
            } else {
                (cursor, anchor)
            };
            Some((start, end))
        }
        _ => None,
    }
};
```

Then for each line in the loop, instead of applying a whole-line style, split the line into up to 3 spans (before-selection, selected, after-selection):

```rust
// Inside the .map(|(i, line)| { ... }) closure, after line_num is computed:

if let Some(((sel_start_row, sel_start_col), (sel_end_row, sel_end_col))) = selection_range {
    if line_num >= sel_start_row && line_num <= sel_end_row {
        // This line is (partially) selected
        let chars: Vec<char> = line.chars().collect();
        let line_len = chars.len();

        let sel_col_start = if line_num == sel_start_row { sel_start_col } else { 0 };
        let sel_col_end = if line_num == sel_end_row { (sel_end_col + 1).min(line_len) } else { line_len };

        // Before selection
        if sel_col_start > 0 {
            let before: String = chars[..sel_col_start].iter().collect();
            spans.push(Span::styled(before, base_style));
        }
        // Selected region
        let selected: String = chars[sel_col_start..sel_col_end].iter().collect();
        spans.push(Span::styled(selected, base_style.add_modifier(Modifier::REVERSED)));
        // After selection
        if sel_col_end < line_len {
            let after: String = chars[sel_col_end..].iter().collect();
            spans.push(Span::styled(after, base_style));
        }

        return Line::from(spans);
    }
}
// Non-selected line: fall through to existing formatting logic
```

**Important details:**
- Tuple comparison `anchor <= cursor` works correctly for `(usize, usize)` — it compares row first, then col. This is exactly the text ordering we want.
- `sel_col_end` is exclusive (hence `+1` on the end col) — the cursor character itself should be included in the selection.
- For lines fully within the selection range (not the first or last line), `sel_col_start = 0` and `sel_col_end = line_len`, so the entire line is reversed.
- This must integrate with the existing `build_line_spans` method for inline formatting (bold/italic) and search highlighting. The simplest approach for v1: when a line has a visual selection, skip `build_line_spans` and just do the 3-span split with a base style. Inline formatting within selected regions is a v2 concern.

### Show cursor position

In the selected region, the character at `visual_cursor` should be visually distinguishable from the rest of the selection. Use `Modifier::UNDERLINED` on just that character:

```rust
// When building the "selected" span for the cursor line:
if line_num == cursor_row {
    // Split the selected span further to underline the cursor char
    // ... (before cursor | cursor char underlined | after cursor)
}
```

This is optional but helpful. Without it, the user can't tell where the cursor is within the selection.

---

## Step 4: Update Text Extraction (Yank)

### File: `src/ui/board.rs` — `get_selected_text()`

**Replace** the current line-joining implementation with character-level extraction:

```rust
pub fn get_selected_text_range(
    &self,
    start: (usize, usize),  // (row, col) inclusive
    end: (usize, usize),    // (row, col) inclusive
) -> String {
    let Some(ts) = &self.text_structure else { return String::new() };
    let (start, end) = if start <= end { (start, end) } else { (end, start) };
    let (start_row, start_col) = start;
    let (end_row, end_col) = end;

    if start_row == end_row {
        // Single line selection
        let chars: Vec<char> = ts.text_lines[start_row].chars().collect();
        return chars[start_col..=end_col.min(chars.len().saturating_sub(1))].iter().collect();
    }

    let mut result = String::new();
    for row in start_row..=end_row {
        if row >= ts.text_lines.len() { break; }
        let chars: Vec<char> = ts.text_lines[row].chars().collect();
        if row == start_row {
            result.extend(&chars[start_col..]);
            result.push('\n');
        } else if row == end_row {
            result.extend(&chars[..=end_col.min(chars.len().saturating_sub(1))]);
        } else {
            result.extend(&chars);
            result.push('\n');
        }
    }
    result
}
```

### File: `src/ui/reader.rs` — `yank_selection()` (~line 2470)

Update to use the new character-level extraction:

```rust
fn yank_selection(&mut self) -> eyre::Result<()> {
    let (anchor, cursor) = {
        let state = self.state.borrow();
        match (state.ui_state.visual_anchor, state.ui_state.visual_cursor) {
            (Some(a), Some(c)) => (a, c),
            _ => return Ok(()),
        }
    };
    let selected_text = self.board.get_selected_text_range(anchor, cursor);
    if !selected_text.is_empty() {
        self.clipboard.set_text(selected_text)?;
        let ui_state = &mut self.state.borrow_mut().ui_state;
        ui_state.set_message("Text copied to clipboard".to_string(), MessageType::Info);
    }
    self.state.borrow_mut().ui_state.open_window(WindowType::Reader);
    Ok(())
}
```

You can keep the old `get_selected_text()` method for backward compatibility or remove it if nothing else calls it.

---

## Step 5: Add Board Helper Methods

### File: `src/ui/board.rs`

Add these helpers on `Board`:

```rust
/// Get the char count of a specific line
pub fn line_char_count(&self, row: usize) -> usize {
    self.text_structure
        .as_ref()
        .and_then(|ts| ts.text_lines.get(row))
        .map(|line| line.chars().count())
        .unwrap_or(0)
}
```

If `self.text_structure` is not `pub`, either make it `pub(crate)` or add a getter:
```rust
pub fn text_structure_ref(&self) -> Option<&TextStructure> {
    self.text_structure.as_ref()
}
```

---

## Step 6: Dictionary Lookup (`d` key)

### File: `src/ui/reader.rs`

Add a new method:

```rust
fn dictionary_lookup(&mut self) -> eyre::Result<()> {
    let (anchor, cursor) = {
        let state = self.state.borrow();
        match (state.ui_state.visual_anchor, state.ui_state.visual_cursor) {
            (Some(a), Some(c)) => (a, c),
            _ => return Ok(()),
        }
    };
    let selected_text = self.board.get_selected_text_range(anchor, cursor);
    let word = selected_text.trim().to_string();
    if word.is_empty() {
        return Ok(());
    }

    // Exit visual mode first
    self.state.borrow_mut().ui_state.open_window(WindowType::Reader);

    // Launch dictionary in background (sdcv for StarDict, or dict, or configurable)
    // Try sdcv first (offline StarDict), fall back to dict
    use std::process::Command;
    let output = Command::new("sdcv")
        .arg("-n")  // non-interactive
        .arg(&word)
        .output()
        .or_else(|_| {
            Command::new("dict")
                .arg(&word)
                .output()
        });

    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout).to_string();
            if text.trim().is_empty() {
                let ui = &mut self.state.borrow_mut().ui_state;
                ui.set_message(format!("No definition found for '{}'", word), MessageType::Info);
            } else {
                // Display in a popup or message
                // For v1: show first ~200 chars as a message
                let preview = if text.len() > 200 { &text[..200] } else { &text };
                let ui = &mut self.state.borrow_mut().ui_state;
                ui.set_message(preview.to_string(), MessageType::Info);
            }
        }
        Err(_) => {
            let ui = &mut self.state.borrow_mut().ui_state;
            ui.set_message("No dictionary program found (install sdcv or dict)".to_string(), MessageType::Info);
        }
    }
    Ok(())
}
```

**Note:** The dictionary feature is a best-effort v1. A proper implementation would show a scrollable popup window (new `WindowType::Dictionary`). For the initial pass, using the existing message system is acceptable. The user may want to customize the dictionary command in config later.

**Alternative (better UX):** Instead of `set_message`, create a new `WindowType::Dictionary` that shows the full definition in a scrollable overlay, similar to how `Help` works. This is more work but much more usable. If you go this route:
1. Add `WindowType::Dictionary` to the enum in `models.rs`
2. Add `dictionary_text: Option<String>` and `dictionary_scroll: u16` to `UiState`
3. Create `src/ui/windows/dictionary.rs` modeled on the help window
4. Handle `j`/`k`/`Esc` for scrolling and closing

---

## Step 7: Header/Status Bar Indicator

Show "-- VISUAL --" in the header or footer when in visual mode, so the user knows the mode is active.

### File: `src/ui/board.rs` or wherever the header is rendered

Check `state.ui_state.active_window == WindowType::Visual` and render a mode indicator. Look at how the existing header/title bar is rendered and add the indicator there. If there's a bottom bar or message area, that's also a good place.

---

## Step 8: Fix Existing Bug

### File: `src/ui/reader.rs` — `open_window()` Visual arm (line 239-241)

The current code:
```rust
WindowType::Visual => {
    let current_row = self.selection_start.unwrap_or(0);
    self.selection_start = Some(current_row);
}
```

This should become a no-op (anchor is set by the caller now):
```rust
WindowType::Visual => {}
```

---

## Summary of All File Changes

| File | Changes |
|---|---|
| `src/models.rs` | (optional) Add `WindowType::Dictionary` to enum |
| `src/ui/reader.rs` (UiState) | Replace `selection_start: Option<usize>` with `visual_anchor: Option<(usize, usize)>` and `visual_cursor: Option<(usize, usize)>` |
| `src/ui/reader.rs` (UiState::new) | Init both to `None` |
| `src/ui/reader.rs` (open_window) | Update Visual arm (no-op), Reader arm (clear both new fields) |
| `src/ui/reader.rs` ('v' handler) | Set anchor and cursor to `(current_row, 0)` before calling `open_window` |
| `src/ui/reader.rs` (handle_visual_mode_keys) | Replace `move_cursor` with `move_visual_cursor`, add `d` key |
| `src/ui/reader.rs` | Add `move_visual_cursor()` method with hjkl + auto-scroll |
| `src/ui/reader.rs` (yank_selection) | Use `get_selected_text_range` with `(row, col)` tuples |
| `src/ui/reader.rs` | Add `dictionary_lookup()` method |
| `src/ui/board.rs` (render_content) | Replace whole-line REVERSED with character-level span splitting |
| `src/ui/board.rs` | Add `get_selected_text_range()`, `line_char_count()`, `text_structure_ref()` |
| `src/ui/board.rs` | Remove or update old `get_selected_text()` |

---

## Verification

1. `cargo build` — must compile without errors
2. `cargo test` — all existing tests must pass
3. Manual test:
   - Open any EPUB: `cargo run -- tests/fixtures/childrens-literature.epub`
   - Navigate to a page with text
   - Press `v` — should see cursor at top-left of viewport
   - Press `l` several times — cursor should move right, highlighting characters
   - Press `j` — cursor moves down, selection expands to include the line
   - Press `k` — cursor moves up, selection shrinks
   - Press `h` — cursor moves left
   - Scroll past page boundary with `j` — page should auto-scroll, selection persists
   - Press `y` — text copies to clipboard, visual mode exits
   - Verify clipboard content with `xclip -selection clipboard -o` or similar
   - Press `v`, select a word, press `d` — dictionary lookup runs
4. Edge cases to test:
   - Select backwards (anchor below cursor)
   - Select on empty lines
   - Select at very beginning/end of book
   - Select across chapter boundaries
