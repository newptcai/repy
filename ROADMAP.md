# repy Feature Roadmap — Toward GUI-Reader Parity

> **This is the canonical progress document.** Feature status, remaining work,
> and deferred hard problems all live here (the old `to-do.md` was merged in
> and removed). Small, insulated tasks are tracked separately as
> `improvements/improvement-XX.md` files per the workflow in CLAUDE.md.

## Context

repy is already strong on the "reading mechanics" side: vim navigation, regex search, visual/cursor mode with motions, context-anchored highlights with comments (`src/annotations.rs`), bookmarks, TTS, jump history, per-book width. To match major GUI readers (Calibre viewer, Thorium, Apple Books, KOReader), the gaps are: in-terminal images, user theming, reading statistics, richer search, real library management, multi-format support, sync, and typography. Several advertised features are also half-wired (CLI `-r`/`-d`, mouse, line numbers, double-spread).

This is a phased roadmap (Phase 1 = high value / low friction → Phases 6–7 = pipeline-perturbing). Each phase can ship as one or more releases per the "commit small and often" project convention.

**Load-bearing architectural constraint** (affects Phases 3 and 6): HTML is flattened to plain-text lines (`html2text`), and all styling/search/highlight/TTS coordinates are `(row, col)` on the wrapped text (`src/parser.rs` → `TextStructure`). Any feature that changes line layout must happen *before* styling recovery in the parse pipeline, and triggers a re-parse (the width-change machinery already handles this pattern). *Update (2026-07): Phase 7 item 0 is relaxing this — the parser now also emits a per-chapter `SourceMap` (wrapped row ↔ char offset into the normalized chapter source), so features migrated onto source offsets survive re-wrapping instead of being recovered from the grid.*

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
3. **Real library** (M-L) — ✅ done (2026-07): `library_directories` setting scanned with `walkdir`; background scan on a worker thread with its own SQLite connection; Calibre folder structure supported through per-book `metadata.opf`; library window merges history with on-disk books, keeps fuzzy filtering, and marks `unread`/`[missing]` entries. Phase 5 subsequently added logical multi-format records, rich Calibre metadata, root-safe caching, series sorting, and explicit refresh/format controls.
4. **Footnote/link popup preview** (S-M) — ✅ done (2026-07): following an internal link opens a ~10-line preview popup (`Enter` jumps, `Esc`/`q` stays), reusing the links-window preview code and covered by TUI snapshots.
5. **Per-book settings** (S) — ✅ done (2026-07): per-book text width is preserved and `reading_states.color_theme` stores an optional book-specific theme override; null inherits the global config theme.

## Phase 3 — In-terminal images — ✅ complete (2026-07)

Crates: `ratatui-image` (kitty graphics / iTerm2 / sixel / halfblocks fallback, `Picker::from_query_stdio()` capability detection) + `image`. **Pin a ratatui-0.30-compatible release** — verify at implementation time. Skip SVG initially.

1. **Full-screen image viewer + library covers first** (M) — ✅ done (2026-07): `Enter` in the images list renders the image full-screen via `ratatui-image` 11 (kitty/iTerm2/sixel/halfblocks; lazy `Picker::from_query_stdio()` in new `src/ui/graphics.rs`), centered with `size_for`; `o` (list or viewer) and SVG fall back to the external viewer; ratatui bumped to 0.30.2 stable. Library window shows the selected book's cover in a side panel (debounced load in the run loop, per-path cache, Calibre `cover.jpg` fast path).
2. **Inline images in reading flow** (L) — ✅ done (2026-07): parser reserves aspect-corrected blank rows per image (capped viewport−2) via per-chapter pixel-dimension prescan; `image_maps` keying unchanged, new `image_block_rows`; reader decodes one visible image per run-loop pass (cached by resolved path) and renders into the block only when fully visible. Setting `inline_images: placeholder | shown` (default placeholder) toggles in the Settings window and re-parses all chapters. `off`/`always` policy variants remain possible follow-ups.

## Phase 4 — Multi-format support — ✅ complete (2026-07)

**4.0 Refactor `Ebook` trait first** (M) — ✅ done (2026-07): trait slimmed to format access (metadata/toc/`get_chapter(index) -> ChapterContent`/`get_resource`/`get_cover`/`content_index_for_href`/`styled_classes`); new `enum ChapterContent { Html, PlainText, Markdown, ImagePage }`; parse orchestration moved to `src/renderer.rs` (`parse_chapter`/`parse_book`, chapter breaks, image-dimension prescan); factory `open(path)` by extension + zip-magic fallback in `src/formats/mod.rs`; `Epub` moved to `src/formats/epub.rs`; reader holds `Box<dyn Ebook>`. `spine_href` kept as the stable chapter ID (content fingerprints unchanged, so book identity is preserved).

Priority order:

1. **Plain text + Markdown** (S) — ✅ done (2026-07): new `src/formats/text.rs` backend (one chapter per file; title from first `# heading` for md, file stem otherwise; Markdown image links resolve against the file's directory). Markdown renders via `pulldown-cmark` (tables/footnotes/strikethrough/tasklists) → HTML → existing pipeline; plain text becomes escaped `<p>` paragraphs split on blank lines. Wired into `formats::open` for `.txt`/`.text`/`.md`/`.markdown`.
2. **FB2** (M) — ✅ done (2026-07): `quick-xml` walk emits top-level sections as HTML chapters, extracts metadata/TOC, supports legacy XML encodings and `.fb2.zip`, and serves base64 inline images and covers through `get_resource`.
3. **MOBI6** (M-L) — ✅ done (2026-07): `mobi` crate backend exposes metadata and HTML content through the shared renderer, maps MOBI `recindex` images to resources, and extracts covers when declared; AZW/AZW3 are documented as best-effort because KF8-only content is outside the crate's MOBI6 support.
4. **CBZ** (M) — ✅ done (2026-07): new `src/formats/cbz.rs` (`zip` crate, deflate only); natural-sorted image entries become one `ImagePage` chapter each, rendered as book-root-relative `<img>` through the existing inline-image pipeline (`inline_images: shown` + graphics terminal to actually see pages); `ComicInfo.xml` supplies title (`Series #Number`) and writer; first page doubles as the cover; `.cbz` included in library scans. CBR skipped (unrar licensing).
5. **PDF — explicitly out of scope** (reflow is a research problem; document "convert with Calibre").

## Phase 5 — Sync and ecosystem

1. **KOReader sync (kosync) client** (M) — shipped **pull-only**: authentication, KOReader partial-MD5 document IDs, background pull, a confirm-jump prompt, and the settings UI. Pull parses the CREngine XPointer KOReader stores in `progress` (`src/xpointer.rs`): `DocFragment[N]` pins the exact chapter and the element path places the reader within it, with a width-independent **content (character) fraction** (`Board::content_fraction`/`row_for_fraction`) as fallback when the pointer is absent or unresolvable. A percentage-plausibility guard rejects DocFragment/spine mismatches. **Deferred:** KOReader-compatible *push* needs a generated XPointer that a wrapped `repy` row cannot supply (it would require reproducing crengine's DOM normalization); until then `repy` never pushes, so it can't corrupt a KOReader bookmark.
2. **Calibre-aware library** (M) — ✅ done (2026-07): schema v7 stores a stable logical-book key, library root, all discovered formats, series/index, tags, language, publisher, description, and cover path. A detected root `metadata.db` is opened through read-only immutable SQLite and supplies the catalog in one pass; incompatible or unavailable databases fall back atomically to sibling `metadata.opf` files and directory scanning. Books are grouped into one row with EPUB-first format preference; `f` cycles formats, `R` refreshes, `c` opens a responsive cover-and-metadata details panel, and `s` includes series sorting. Fuzzy search covers title, author, series, tags, and path. Selection survives refresh/sort. Cache invalidation tracks database, ebook, OPF, and cover mtimes; pruning is isolated per successfully scanned root, unavailable roots retain cached entries, directory symlinks are not followed, and canonical paths deduplicate overlapping roots. The Calibre database and files are never written.
3. **OPDS catalog browsing** (M) — ✅ done (2026-07): OPDS 1.2 Atom browsing,
   OpenSearch, pagination, Basic auth with origin isolation, background
   validated downloads, and direct open. The protocol-neutral model leaves
   OPDS 2.0 as an additional JSON parser.

## Phase 6 — Typography — ✅ complete (2026-07)

All changes go inside/around `wrap_text` in `src/parser.rs` *before* styling recovery; bundle into one parser-touching release.

1. **Paragraph spacing** and **line spacing 1.5/2.0** — ✅ done: global paragraph-style and line-spacing controls insert layout rows before coordinate recovery; vertical motions skip generated gaps and changes trigger a full-book re-parse.
2. **Justification** — ✅ done: display-width-aware space distribution skips structural, centered, CJK-only, and paragraph-final lines while recovered formatting and links retain correct coordinates.
3. **First-line indent** mode — ✅ done: the `indented` paragraph style removes prose gaps and applies a two-column first-line indent; `compact` provides the same gapless layout without indentation.

## Phase 7 — Layout-independent coordinates and deferred hard problems

Phase 6 exposed the structural limit of the current pipeline: semantics (search hits, styling, links, selections) are *recovered from* the rendered `(row, col)` grid, so every layout feature that perturbs whitespace degrades them. This phase moves the hard consumers onto layout-independent text. Items 0–3 share machinery; item 4 is an independent research problem.

0. **Canonical source coordinates (`SourceMap`)** (L) — ✅ complete (2026-07). The shared machinery for items 1–3: the parser now *produces* per-chapter source spans during wrapping instead of features recovering positions by text-matching afterwards. `TextStructure.source_map` maps every wrapped row to a `[start, end)` char span of the normalized chapter source text (`annotations::normalize_text` over the pre-wrap `raw_lines`), with `offset_for_row`/`row_for_offset` projection both ways. Shipped as four commits:
   - (a) ✅ parser emits `SourceMap`; pagebreaks anchored by offset; span-invariant and cross-width round-trip tests.
   - (b) ✅ reading position/width-rebuild on source offsets (`reading_states.source_offset`, schema v8; restore ladder falls back to same-width raw row, then `rel_pctg`, then clamp for legacy databases).
   - (c) ✅ bookmarks, marks, and jump history on source offsets (jump history stores full states; jumps resolve through the restore ladder).
   - (d) ✅ links/sections/footnotes anchored by offset at parse time (retired `find_line_by_words`; fixed links lost to hyphen-wrapped labels).
1. **Layout-independent search** (was M-L, now S-M on top of item 0; split into `improvements/improvement-09.md` (engine swap) and `improvement-10.md` (range-aware navigation/rendering)) — 🚧 engine swap complete (2026-07): regexes now run once per chapter over `source_map.source_text`, so phrases survive justification, wrap boundaries, and display hyphenation. Each source hit is projected back to character-column ranges on every touched rendered row, remains one results-window entry anchored at its first row, carries its canonical `(content_index, start, end)` coordinates for the next slice, and uses a source-text context preview. Search and visual-mode highlight ranges now share character units, fixing their previous byte/character mismatch on non-ASCII lines. **Remaining:** `improvement-10.md` makes current-hit styling and `n`/`p` navigation range-aware across all rows of a multi-row hit.
2. **Whitespace-robust styling recovery** (M; short-term fix split out as `improvements/improvement-06.md`, matcher migration stays here) — `match_sequence` in `src/parser.rs` scans a fixed 20-byte `lookahead_limit` for the next token, so a sparsely justified line (two words, wide gap) silently drops its bold/italic coordinates. Short-term: make the lookahead skip whitespace runs before counting. Real fix: migrate the styling matcher onto the item-0 source spans, retiring the matcher family. (The link half of the original item is covered by item 0 step (d).)
3. **Normalized selection and TTS output** (S) — ✅ complete (2026-07), shipped as `improvements/improvement-07.md` (yank + dictionary, shared rendered-column/source-offset mapping) and `improvement-08.md` (TTS). Yanked text, dictionary queries, and TTS chunks now read clean text from `source_map.source_text`; TTS underline ranges are projected back onto rendered rows, preserving highlighting across justification, first-line indents, and wrap hyphens.
4. **KOReader-compatible push sync** (L) — deferred from Phase 5. Requires generating a crengine-compatible XPointer for an arbitrary reading position, i.e. reproducing crengine's DOM normalization closely enough that KOReader resolves it; getting it wrong corrupts the user's KOReader bookmark, so it must ship behind an opt-in setting and be verified against a real KOReader install on the same files.
5. **Smaller deferred follow-ups** (S-M, independent) — OPDS 2.0 JSON catalogs (Phase 5 note), `inline_images: off`/`always` policy variants (Phase 3 note), more built-in themes (Solarized, Nord, Catppuccin; Phase 1 note), KF8-only AZW3 support if a viable crate appears (Phase 4 note), platform packages via `cargo-deb`/`cargo-wix` on top of the existing release workflow (from the old to-do.md; CI already builds Linux/Windows/macOS binaries in `.github/workflows/release.yml`).

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
