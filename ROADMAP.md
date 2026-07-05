# repy Feature Roadmap — Toward GUI-Reader Parity

## Context

repy is already strong on the "reading mechanics" side: vim navigation, regex search, visual/cursor mode with motions, context-anchored highlights with comments (`src/annotations.rs`), bookmarks, TTS, jump history, per-book width. To match major GUI readers (Calibre viewer, Thorium, Apple Books, KOReader), the gaps are: in-terminal images, user theming, reading statistics, richer search, real library management, multi-format support, sync, and typography. Several advertised features are also half-wired (CLI `-r`/`-d`, mouse, line numbers, double-spread).

This is a phased roadmap (Phase 1 = high value / low friction → Phase 6 = pipeline-perturbing). Each phase can ship as one or more releases per the "commit small and often" project convention.

**Load-bearing architectural constraint** (affects Phases 3 and 6): HTML is flattened to plain-text lines (`html2text`), and all styling/search/highlight/TTS coordinates are `(row, col)` on the wrapped text (`src/parser.rs` → `TextStructure`). Any feature that changes line layout must happen *before* styling recovery in the parse pipeline, and triggers a re-parse (the width-change machinery already handles this pattern).

Effort estimates: S = small, M = medium, L = large.

---

## Phase 1 — Finish half-wired features + daily-use polish

1. **CLI completion** (S) — implement `-r` (print library + progress, exit), `-d` (dump parsed text to stdout, bypass TUI), and positional `EBOOK` as history-number or fuzzy pattern (epy's signature launch UX). Files: `src/main.rs`, `src/cli.rs`; reuse `State` library queries in `src/state.rs`.
2. **Fix image MIME + cover extraction** (S) — `get_img_bytestr` (`src/ebook.rs`) hardcodes `image/jpeg`; real MIME is already in `doc.resources`. Add `cover()` to the `Ebook` trait (epub crate has `get_cover()`). Prerequisite for Phase 3.
3. **User-definable themes** (M) — replace closed `ColorTheme` enum (`src/theme.rs`) with named themes: config JSON maps the semantic slots to color strings (ratatui `Color: FromStr` handles `#hex`/names). Ship more built-ins: Solarized, Nord, Catppuccin, and a **sepia/paper** theme (flagship reading mode of Apple Books/KOReader). Files: `src/theme.rs`, `src/config.rs`, `src/settings.rs`.
4. **Search upgrades** (M) — distinct current-hit style (new theme slots), match counter "14/87" in status bar, search history (persisted, arrows in prompt), incremental search (re-run regex per keystroke over in-memory lines, ~50ms debounce). Files: `src/ui/reader/mod.rs`, `src/ui/board.rs`, `src/state.rs` (migration).
5. **Fuzzy filtering in TOC/library/bookmarks/highlights windows** (S-M) — `/` to filter, crate `nucleo-matcher`. Shared widget in `src/ui/windows/mod.rs`.
6. **Highlight colors, Markdown export, margin indicators** (M) — schema already has `highlights.color`; add color cycle on create/edit (KOReader's 5-color set, per-theme tuned RGB); `--export-highlights --format md|json` grouped by chapter; 1-col gutter with colored `▎` on highlighted rows. Files: `src/ui/reader/mod.rs`, `src/ui/board.rs`, `src/main.rs`.
7. **Mouse + line-number wiring** (S-M) — honor `mouse_support` (wheel scroll, click-to-follow/select; only enable capture when the setting is on so native terminal copy still works); fix `show_line_numbers` gutter not being subtracted from text width (`src/ui/board.rs`).
8. **Double-spread: implement minimally or delete** (M) — two columns at wide terminals (left = rows `k..k+h`, right = `k+h..k+2h`); visual mode/TTS temporarily drop to single column. If the compromise proves ugly, delete the settings instead of leaving them half-wired.

## Phase 2 — Data layer: statistics, persistence, library

1. **Reading statistics** (M) — *do early: data only becomes valuable once it accumulates.* New `sessions` table (book_id, start/end time, rows, words); idle detection closes sessions (KOReader semantics). New Statistics window: per-book time/words/wpm/est. time left, global totals + streaks; status-bar "~34 min left in chapter". Files: `src/state.rs` (migration), `src/ui/reader/mod.rs` (event hooks), new `src/ui/windows/statistics.rs`.
2. **Persist jump history + marks per book** (S) — currently in-memory only.
3. **Real library** (M-L) — configurable scan directories (`walkdir`), background metadata scan cached in SQLite (path+mtime), sort by recent/title/author/progress, fuzzy filter, distinguish on-disk vs history. Rewrite `src/ui/windows/library.rs`; follow the TTS worker-thread pattern for background scanning.
4. **Footnote/link popup preview** (S-M) — on following an internal link, show ~10-line preview popup ("Enter = jump, Esc = stay") instead of jumping away; reuse the links-window preview code. Worst reading interruption vs GUI readers today.
5. **Per-book settings** (S) — extend per-book persistence (width exists) to theme, later spacing/justification; nullable columns = inherit global.

## Phase 3 — In-terminal images

Crates: `ratatui-image` (kitty graphics / iTerm2 / sixel / halfblocks fallback, `Picker::from_query_stdio()` capability detection) + `image`. **Pin a ratatui-0.30-compatible release** — verify at implementation time. Skip SVG initially.

1. **Full-screen image viewer + library covers first** (M) — fixed Rect = no scrolling pain; fall back to external viewer when the terminal lacks graphics support. Files: `src/ui/windows/images.rs`, `src/ui/windows/library.rs`, new `src/ui/graphics.rs` (Picker + protocol cache).
2. **Inline images in reading flow** (L) — at parse time reserve N blank rows per image (aspect-corrected, capped at viewport−2); `image_maps` row keying unchanged; render cached protocol into the block's Rect when visible. Policy setting: `off | placeholder | shown-when-fully-visible | always` (start with fully-visible-only — clean on all backends). Mode change triggers re-parse (existing machinery). Highlights survive: they're context-anchored, not row-anchored.

## Phase 4 — Multi-format support

**4.0 Refactor `Ebook` trait first** (M) — split format access from rendering: trait keeps metadata/toc/`get_chapter(index) -> ChapterContent`/`get_resource`/`cover`; new `enum ChapterContent { Html, PlainText, Markdown, ImagePage }`; move parse orchestration into a renderer layer; factory `open(path)` by extension+magic in new `src/formats/mod.rs`, move `Epub` to `src/formats/epub.rs`. Keep `spine_href` as the stable chapter ID (highlight anchoring and book identity depend on it).

Priority order:

1. **Plain text + Markdown** (S) — Markdown via `pulldown-cmark` → HTML → existing pipeline (nearly free).
2. **FB2** (M) — `quick-xml` walk emitting HTML-ish chapters; base64 inline images through `get_resource`.
3. **MOBI6** (M-L) — crate `mobi`; AZW3/KF8 documented as best-effort.
4. **CBZ** (M) — `zip` crate, `ImagePage` chapters; gated on Phase 3 + kitty-class terminal. Skip CBR (unrar licensing).
5. **PDF — explicitly out of scope** (reflow is a research problem; document "convert with Calibre").

## Phase 5 — Sync and ecosystem

1. **KOReader sync (kosync) client** (M) — minimal HTTP+JSON protocol (register/auth with MD5 password header, PUT/GET `/syncs/progress`); document ID must match KOReader's partial-MD5 algorithm exactly. Sync on percentage. New `src/sync.rs`, crate `md-5`, reuse blocking `reqwest` on a worker thread; push on close, pull on open with a "device X is further ahead — jump?" prompt.
2. **Calibre library read** (S-M) — detect `metadata.db` in library paths, read books/authors/series/tags via bundled `rusqlite` (read-only + immutable flag). Never write.
3. **OPDS catalog browsing** (M, optional) — Atom via `quick-xml`, download + open; parity with Thorium.

## Phase 6 — Typography (do last: perturbs the row-keyed coordinate system)

All changes go inside/around `wrap_text` in `src/parser.rs` *before* styling recovery; bundle into one parser-touching release.

1. **Paragraph spacing** (S) and **line spacing 1.5/2.0** (M) — insert blank lines pre-recovery; j/k skip blanks; toggle triggers re-parse.
2. **Justification** (M) — post-wrap space distribution before styling recovery; skip code/centered/CJK-only lines and paragraph-final lines.
3. **First-line indent** mode (S, optional) — classic book typography (indent + no blank line).

## Explicitly skipped (poor TUI fits)

Fonts/sizes (terminal owns the font) · complex CSS layout (plain-text reflow *is* the product) · fixed-layout EPUB (detect and warn) · DRM · PDF · page-turn animation beyond existing setting · Calibre DB writing.

## New crates

`nucleo-matcher`, `walkdir`, `ratatui-image` + `image` (pin for ratatui 0.30 beta), `pulldown-cmark`, `quick-xml`, `mobi`, `zip`, `md-5`, optionally `infer`. No async runtime — blocking + worker threads matches the existing TTS pattern.

## Verification approach (per feature)

- Follow existing conventions: unit tests inline in modules, integration tests in `tests/` with fixtures (`tests/fixtures/small.epub`, `meditations.epub`); minimal HTML fixtures for parser changes per CLAUDE.md.
- SQLite migrations: test upgrade from a copy of an existing `states.db`.
- Image rendering: manual matrix across kitty / foot (sixel) / plain xterm (halfblocks/fallback).
- Kosync: test against a self-hosted kosync server with a throwaway account; verify document-ID matches KOReader on the same file.
- CLI: `assert_cmd` tests for `-r`/`-d`/pattern launch (pattern exists in `tests/cli.rs`).
- Each keybinding change updates the help window + README in the same commit (project rule).
