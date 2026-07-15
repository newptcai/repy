# repy Feature Roadmap — Toward GUI-Reader Parity

## Context

repy is already strong on the "reading mechanics" side: vim navigation, regex search, visual/cursor mode with motions, context-anchored highlights with comments (`src/annotations.rs`), bookmarks, TTS, jump history, per-book width. To match major GUI readers (Calibre viewer, Thorium, Apple Books, KOReader), the gaps are: in-terminal images, user theming, reading statistics, richer search, real library management, multi-format support, sync, and typography. Several advertised features are also half-wired (CLI `-r`/`-d`, mouse, line numbers, double-spread).

This is a phased roadmap (Phase 1 = high value / low friction → Phase 6 = pipeline-perturbing). Each phase can ship as one or more releases per the "commit small and often" project convention.

**Load-bearing architectural constraint** (affects Phases 3 and 6): HTML is flattened to plain-text lines (`html2text`), and all styling/search/highlight/TTS coordinates are `(row, col)` on the wrapped text (`src/parser.rs` → `TextStructure`). Any feature that changes line layout must happen *before* styling recovery in the parse pipeline, and triggers a re-parse (the width-change machinery already handles this pattern).

Effort estimates: S = small, M = medium, L = large.

---

## Phase 1 — Finish half-wired features + daily-use polish — ✅ complete (2026-07)

1. **CLI completion** (S) — ✅ done: `-r`, `-d`, and history-number/pattern launch.
2. **Fix image MIME + cover extraction** (S) — ✅ done: manifest-based MIME with extension fallback; `get_cover()` on the `Ebook` trait. Prerequisite for Phase 3.
3. ~~**User-definable themes**~~ — dropped by decision (2026-07): built-in themes only. A sepia/paper theme was added to the built-in cycle instead. More built-ins (Solarized, Nord, Catppuccin) remain possible as S-effort additions to `src/theme.rs`.
4. **Search upgrades** (M) — ✅ done: distinct current-hit style, `match N/M` counter, persisted search history (Up/Down in prompt, capped at 100), incremental search with Esc-restore.
5. **Fuzzy filtering in TOC/library/bookmarks/highlights windows** (S-M) — ✅ done: `/` filters with `nucleo-matcher`; shared helper in `src/ui/windows/mod.rs`; Esc clears, Enter acts on the selection.
6. **Highlight colors, Markdown export, margin indicators** (M) — ✅ done: five-color highlights with `C` cycling, `--export-highlights --format md`, and a 1-col margin gutter with colored `▎` on highlighted rows.
7. **Mouse + line-number wiring** (S-M) — ✅ done: gutter width fix; `mouse_support` honored (capture only when on, live toggle in Settings, wheel scroll everywhere, click-to-follow links in the reader).
8. ~~**Double-spread: implement minimally or delete**~~ — resolved by deletion (2026-07): the half-wired settings (`start_with_double_spread`, `double_spread_toggle`, `DoubleSpreadPadding`) were removed. A real two-column mode would perturb the row-keyed coordinate system (visual mode, TTS, highlights, search) for modest value in a TUI; can be revisited as its own project if ever wanted.

## Phase 2 — Data layer: statistics, persistence, library — ✅ complete (2026-07)

1. **Reading statistics** (M) — ✅ done (2026-07): new `reading_sessions` table keyed by stable `book_id`; active sessions close on idle, book switch, and quit; Statistics window shows per-book/global time, rows, words, WPM, estimates, and streaks; the top bar shows estimated chapter time remaining.
2. **Persist jump history + marks per book** (S) — ✅ done (2026-07): new `jump_history` and `marks` tables; Vim-style `m<c>` / `` `<c> `` marks persist per book; jump history is saved on state persistence.
3. **Real library** (M-L) — ✅ done (2026-07): `library_directories` setting scanned with `walkdir` (symlinks followed); background scan on a worker thread with its own SQLite connection, cached in `library_files` by path+mtime; Calibre folder structure supported by reading the per-book `metadata.opf` (no EPUB unzip, no Calibre DB writes); library window merges history with on-disk books, sorts by recent/title/author/progress (`s`), keeps fuzzy filter, and marks `unread`/`[missing]` entries.
4. **Footnote/link popup preview** (S-M) — ✅ done (2026-07): following an internal link opens a ~10-line preview popup (`Enter` jumps, `Esc`/`q` stays), reusing the links-window preview code and covered by TUI snapshots.
5. **Per-book settings** (S) — ✅ done (2026-07): per-book text width is preserved and `reading_states.color_theme` stores an optional book-specific theme override; null inherits the global config theme.

## Phase 3 — In-terminal images — ✅ complete (2026-07)

Crates: `ratatui-image` (kitty graphics / iTerm2 / sixel / halfblocks fallback, `Picker::from_query_stdio()` capability detection) + `image`. **Pin a ratatui-0.30-compatible release** — verify at implementation time. Skip SVG initially.

1. **Full-screen image viewer + library covers first** (M) — ✅ done (2026-07): `Enter` in the images list renders the image full-screen via `ratatui-image` 11 (kitty/iTerm2/sixel/halfblocks; lazy `Picker::from_query_stdio()` in new `src/ui/graphics.rs`), centered with `size_for`; `o` (list or viewer) and SVG fall back to the external viewer; ratatui bumped to 0.30.2 stable. Library window shows the selected book's cover in a side panel (debounced load in the run loop, per-path cache, Calibre `cover.jpg` fast path).
2. **Inline images in reading flow** (L) — ✅ done (2026-07): parser reserves aspect-corrected blank rows per image (capped viewport−2) via per-chapter pixel-dimension prescan; `image_maps` keying unchanged, new `image_block_rows`; reader decodes one visible image per run-loop pass (cached by resolved path) and renders into the block only when fully visible. Setting `inline_images: placeholder | shown` (default placeholder) toggles in the Settings window and re-parses all chapters. `off`/`always` policy variants remain possible follow-ups.

## Phase 4 — Multi-format support

**4.0 Refactor `Ebook` trait first** (M) — ✅ done (2026-07): trait slimmed to format access (metadata/toc/`get_chapter(index) -> ChapterContent`/`get_resource`/`get_cover`/`content_index_for_href`/`styled_classes`); new `enum ChapterContent { Html, PlainText, Markdown, ImagePage }`; parse orchestration moved to `src/renderer.rs` (`parse_chapter`/`parse_book`, chapter breaks, image-dimension prescan); factory `open(path)` by extension + zip-magic fallback in `src/formats/mod.rs`; `Epub` moved to `src/formats/epub.rs`; reader holds `Box<dyn Ebook>`. `spine_href` kept as the stable chapter ID (content fingerprints unchanged, so book identity is preserved).

Priority order:

1. **Plain text + Markdown** (S) — ✅ done (2026-07): new `src/formats/text.rs` backend (one chapter per file; title from first `# heading` for md, file stem otherwise; Markdown image links resolve against the file's directory). Markdown renders via `pulldown-cmark` (tables/footnotes/strikethrough/tasklists) → HTML → existing pipeline; plain text becomes escaped `<p>` paragraphs split on blank lines. Wired into `formats::open` for `.txt`/`.text`/`.md`/`.markdown`.
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
