# Julia Invocation Approval

Always request user approval before running any `julia` command so the harness can escalate out of the restrictive sandbox.

# Git Repository Guidelines

## Commit Message Guidelines (Codex CLI)
- Use GitHub emoji style subjects in imperative voice, ≤72 chars.
  - Format: `EMOJI (scope): Short imperative subject`.
  - Scope: usually a script name (kebab-case) or area like `img`, `audio`, `net`.
  - Examples: `✨ (mdview.sh): Add GUI preview via xdg-open`, `🐛 (img-trim.sh): Fix threshold for white borders`.
  - Common emojis:
    - ✨: feature/new option
    - 🐛: bug fix
    - 📝: docs
    - ♻️: refactor
    - 🎨: style/formatting
    - ⚡: performance
    - ✅: tests
    - 🔧: config/chore
    - 🚚: move/rename
    - 🔥: remove code
- Keep subjects focused and group related script updates in one commit to avoid unrelated churn.
- Bodies: never embed literal "\n"; use multiple `-m` flags (each becomes a paragraph) or a here-doc to build multi-line messages. Prefer bullets or short paragraphs instead of inlined `\n` escape sequences.
- Safe patterns:
  - `git commit -m "✨ (tool): Add feature" -m "- First bullet" -m "- Second bullet"`
  - `git commit -F - <<'MSG'
    ✨ (tool): Add feature

    - First bullet
    - Second bullet
    MSG`
- Amending safely: `git commit --amend -m "SUBJECT" -m "Bullet 1" -m "Bullet 2"`.

## Pull Request Guidelines
- Include purpose, sample commands, expected/actual behavior, and any external tool requirements (e.g., `ffmpeg`, `ImageMagick`, `pdftk`). Add before/after snippets or file counts when relevant.

Example
```
✨ (vim): Add lexima rules for TeX

- Add $, \(\), and \[\ \\] pairing rules
- Guard Markdown vimtex init behind exists() check
```

## Gemini Added Memories
- For true or false problems, consult `templates/sample-true-false-problem.md` for the correct coding pattern (specifically using `statement_pool` and printing prompts directly).
- Prefer using `\bfq`, `\bfv`, etc., over `\mathbf{q}`, `\mathbf{v}` for bold vectors in LaTeX/Quarto files.
- In each subproblem of a QMD file, there should be exactly one `long_answer` or `short_answer` call.
- A problem's title in a QMD file should match the name of the section it belongs to.
- When using `short_answer` and "no prompt" is desired, provide only the solution LaTeXString. This string will then serve as the label for the answer box.
- When a string does not contain math expressions (in $...$) use "..." instead of L"..."
- New problem files should include "-v1" in their filenames (e.g., "ex-19-v1.qmd").
- When turning a string `s` into a `LaTeXString`, use `LaTeXString(s)` instead of `latexstring(s)`.
- Always write \mathcal{B} in subscript as _{\mathcal{B}}.

## Development Guidelines
- AGENTS.md, GEMINI.md, and CLAUDE.md are the same file; changing one updates the other two automatically.
- When changing shortcuts or keybindings, always update the in-app help window and README.md in the same change.
- Commit frequently with small, focused changes.
- Test-driven development for complex components.
- Cross-platform mindset (Linux/macOS/Windows).
- Preserve epy behavior while improving performance.
- Always update `Cargo.lock` with `cargo update` before tagging a release to avoid CI failures.

## Codex Improvement Tasks

Small, insulated improvement tasks live in `improvements/improvement-XX.md`.
When asked to implement one of these tasks (typically as Codex running in
full-auto mode), follow these rules:

- Implement exactly one task per run. Read the whole task file first; stay
  inside its stated scope and "Out of scope" limits.
- You own the full slice: implementation, tests, docs. Do not leave TODOs or
  half-wired code paths.
- Before committing, all of these must pass:
  - `cargo fmt --all` (then check nothing is left dirty by formatting)
  - `cargo clippy --all-targets` with no new warnings
  - `cargo test`
- Snapshot tests: never run `cargo insta review` (interactive). For
  intentional snapshot changes or new snapshots, run tests then
  `cargo insta accept`, and eyeball the accepted `.snap` diffs yourself.
- Keybinding or UI changes must update the in-app help window and README.md in
  the same commit (project rule above).
- When the task is complete and green, change the task file's `Status:` line
  from `todo` to `done` and commit everything per the commit message
  guidelines in this file (one focused commit is fine; two if code + docs
  separation is clearly cleaner).
- Do not bump the version or tag releases as part of an improvement task.

## Release Process
1. Bump version in `Cargo.toml` (minor bump: 0.8.x → 0.9.0; patch bump: 0.8.x → 0.8.(x+1)).
2. Run `cargo update` to refresh `Cargo.lock`.
3. Commit: `🔧 (repy): Bump version to X.Y.Z`
4. Tag: `git tag vX.Y.Z && git push --tags`


## Success Metrics
- Feature parity with epy
- Performance improvement (per-chapter caching implemented)
- Memory efficiency
- Reliability (comprehensive test suite)
- Maintainability (clean module structure)

## Architecture Overview

### Project Structure
```
src/
├── main.rs              # Entry point, terminal setup
├── cli.rs               # Command-line argument parsing
├── config.rs            # Configuration loading/saving (XDG support)
├── state.rs             # SQLite database for reading state, library, bookmarks
├── formats/
│   ├── mod.rs           # Ebook trait, ChapterContent enum, open() factory
│   ├── epub.rs          # EPUB format backend (epub crate)
│   ├── text.rs          # Plain-text/Markdown backend (single-chapter files)
│   └── cbz.rs           # Comic-book archive backend (zip of image pages)
├── renderer.rs          # ChapterContent → TextStructure (parse orchestration)
├── library.rs           # Library scanning (Calibre metadata.db/OPF + walkdir fallback)
├── parser.rs            # HTML-to-text conversion, wrapping, hyphenation
├── models.rs            # Data models (ReadingState, SearchData, WindowType, etc.)
├── settings.rs          # User settings structure
├── logging.rs           # Debug logging utilities
├── theme.rs             # Color theme definitions for the TUI
├── ui/
│   ├── mod.rs           # UI module root
│   ├── reader/
│   │   └── mod.rs       # Main reader state, event handling, and TTS pipeline
│   ├── board.rs         # Rendering logic for the reading view
│   ├── graphics.rs      # Terminal graphics detection (ratatui-image Picker)
│   └── windows/
│       ├── mod.rs       # Windows module root, shared helpers (centered_popup_area)
│       ├── bookmarks.rs
│       ├── dictionary.rs
│       ├── help.rs
│       ├── images.rs
│       ├── library.rs
│       ├── links.rs
│       ├── metadata.rs
│       ├── search.rs
│       ├── settings.rs
│       └── toc.rs
└── lib.rs               # Library crate root
```

### Key Modules

**`src/config.rs`** (~580 lines):
- Config file discovery with XDG_CONFIG_HOME support
- JSON-based configuration with serde deserialization
- Settings and keymap management

**`src/state.rs`** (~800 lines):
- SQLite database schema: `reading_states`, `library`, `bookmarks`
- Bundled SQLite (no system dependency required)
- CRUD operations for all state types

**`src/formats/`**:
- `Ebook` trait: format access only — metadata, TOC, `get_chapter(index) -> ChapterContent`, `get_resource`, cover; `spine_href` is the stable chapter ID (highlight anchoring and book identity depend on it)
- `ChapterContent` enum: `Html | PlainText | Markdown | ImagePage` raw payloads
- `open(path)` factory picks the backend by extension with a zip-magic fallback
- `epub.rs`: EPUB backend using the `epub` crate (spine filtering, NCX/nav TOC, CSS-derived styled classes)
- `text.rs`: plain-text/Markdown backend — one chapter per file, title from the first `# heading` (md) or file stem, relative image links resolved against the file's directory
- `cbz.rs`: comic-book archive backend — natural-sorted image entries become one `ImagePage` chapter each; title/author from `ComicInfo.xml` when present; first page is the cover; readable with `inline_images: shown` on a graphics terminal

**`src/renderer.rs`**:
- Turns `ChapterContent` into wrapped `TextStructure`s via the shared HTML parse pipeline (`parse_chapter`, `parse_book`)
- Converts non-HTML payloads to minimal HTML first; owns chapter-break padding and inline-image dimension prescans

**`src/parser.rs`** (~1400 lines):
- HTML-to-text conversion using html2text and scraper
- Hyphenation using the `hyphenation` crate (embedded en-US dictionary)
- Text wrapping with `textwrap`
- Link extraction and resolution
- Footnote handling and backlink filtering
- Image placeholder generation
- Inline formatting (bold/italic marker stripping)
- Per-chapter TextStructure caching for fast re-rendering

**`src/ui/reader/mod.rs`** (~4600 lines):
- ApplicationState management (reading state, UI state, config, search)
- Event handling for all keybindings
- Window state machine (Reader, Help, Toc, Bookmarks, etc.)
- Jump history implementation (Ctrl+o/Ctrl+i)
- Visual mode for text selection and yanking
- Search navigation (n/p/N)
- Width adjustment with database persistence
- Wikipedia lookup for dictionary definitions
- TTS chunking, underline tracking, and background audio prefetch for file-based engines

**`src/ui/board.rs`** (~400 lines):
- Rendering the reading view with ratatui
- Syntax highlighting for search results
- Line number display (toggleable)
- Progress indicator in header

**`src/ui/windows/*.rs`**:
- Modular window implementations (each ~50-400 lines)
- Consistent interface: render() and event handling
- Shared `centered_popup_area` helper in `mod.rs`
- Windows: Help, ToC, Bookmarks, Library, Search, Links, Images, Metadata, Settings, Dictionary

### Testing Strategy
- Integration tests in `tests/` directory
- Test fixtures in `tests/fixtures/`
- Tests cover: CLI args, EPUB loading, parser correctness, footnotes, images, jump history, hyphenation, config, models, settings, state, board, dictionary, Wikipedia lookup
- Current coverage: 166+ test cases across unit and integration tests

#### Testing HTML Parsing Issues
When debugging or fixing HTML parsing bugs (links, footnotes, sections, formatting):
1. **Create a minimal HTML fixture** in `tests/fixtures/` that reproduces the issue — trim the source HTML to only the essential structure (a few paragraphs + the problematic elements).
2. **Write unit tests** in `src/parser.rs` `mod tests` that call the internal functions directly (`extract_links`, `extract_sections`, `extract_formatting`, `find_line_by_words`, etc.) with `Html::parse_document(&html)` or `Html::parse_fragment(&html)`.
3. **Provide explicit `text_lines`** that match what the parser would produce — this avoids coupling to the full `parse_html` pipeline and keeps tests fast and focused.
4. See `tests/fixtures/footnotes-class-based.html` and the `test_extract_links_filters_class_based_backlinks` / `test_extract_sections_class_based_footnotes` tests for a reference pattern.

#### TUI Snapshot Testing (rendered-output tests)
`Reader` is generic over the ratatui `Backend` trait, so UI behavior is tested end-to-end in-process with `TestBackend` + `insta` snapshots — no real terminal needed:
- Tests live in `src/ui/reader/snapshot_tests.rs` (`#[cfg(test)]` submodule of the reader, so private items are accessible); snapshots in `src/ui/reader/snapshots/` (committed).
- Harness: `Reader::with_backend(config, TestBackend::new(80, 24), State::new_for_test())`, `load_ebook(tests/fixtures/small.epub)`, then feed `KeyEvent`s via `press`/`press_char`/`type_str` helpers and `insta::assert_snapshot!(reader.terminal.backend())`.
- When a rendering change breaks snapshots intentionally, review and accept with `cargo insta review` (or `INSTA_UPDATE=always cargo test` then inspect the diff).
- Determinism rules: fixed 80×24 size, fixed fixture, call `ui_state.clear_message()` after actions that queue status toasts (messages can embed absolute paths).
- Note: `TestBackend`'s Display renders characters only, not colors/styles — assert on structure (cursor position, counters, layout), not highlight colors.
- Add a snapshot test whenever a new window or reader-mode rendering path is introduced.

### Dependencies
- `ratatui` 0.30.2: Terminal UI framework
- `ratatui-image` 11 (no default features — avoids the chafa C dependency): In-terminal image rendering
- `image` 0.25: Image decoding for the in-terminal viewer
- `crossterm` 0.29.0: Cross-platform terminal manipulation
- `epub` 2.1.5: EPUB file parsing
- `rusqlite` 0.37.0 (bundled): SQLite database
- `scraper` 0.24.0: HTML parsing
- `hyphenation` 0.8.4 (embed_en-us): Hyphenation engine
- `textwrap` 0.16.1: Text wrapping with hyphenation support
- `clap` 4.5.53: CLI argument parsing
- `pulldown-cmark` 0.13: Markdown-to-HTML conversion for .md books
- `zip` 8 (deflate only): CBZ comic archive reading
- `arboard` 3.6.1: Clipboard access for yank functionality
- `walkdir` 2.5.0: Recursive library directory scanning

## Feature Details

### Library
- Press `r` to open the Library window: reading history merged with books
  found in the configured `library_directories` (set in `configuration.json`,
  `~` expands to home)
- Background scan on a worker thread (own SQLite connection), signalled over
  an mpsc channel polled in the main event loop; results cached in the
  `library_files` table keyed by (canonical path, mtime)
- Calibre libraries use a read-only immutable `metadata.db` catalog when
  available, with per-book `metadata.opf` plus directory walking as the
  schema-tolerant fallback; Calibre files and its database are never written
- `s` cycles sorting (recent/title/author/progress), `/` fuzzy-filters,
  `d` removes a history entry; never-opened books show `unread`, history
  entries whose file vanished show `[missing]`
- The selected book's cover renders in a side panel when the terminal
  supports graphics: loading is debounced (150 ms) in the run loop so
  scrolling stays responsive, cached per filepath on the `Reader`, and a
  Calibre-style sibling `cover.jpg` is preferred over unzipping the EPUB

### Image Handling
- Images are preprocessed to include descriptive alt text (e.g., `[Image: filename.jpg]`).
- Image placeholders are centered in the reader view.
- With `"inline_images": "shown"` (settable in the Settings window; default
  `"placeholder"`), images render inline in the reading flow: the parser
  reserves aspect-corrected blank rows under each placeholder
  (`TextStructure.image_block_rows`, capped at viewport−2) using pixel
  dimensions prescanned per chapter, and the reader decodes visible images
  one per run-loop pass (cached by resolved path) and renders them into
  their blocks — only when the block is fully on screen. A partially
  visible block shows its placeholder line as a marker instead. Page
  up/down moves that would start the window inside a block snap so the
  block lands fully on the page (forward: block top at window top;
  backward: block bottom at window bottom), so full-page images are never
  skipped over as blank screens. Toggling the setting (or a
  viewport-height change while `shown`) re-parses all chapters; width
  changes keep the single-chapter fast path.
- SVG-wrapped raster images (`<svg><image xlink:href=…/></svg>`, the
  common Calibre cover pattern) are normalized to plain `<img>` tags and
  flow through the same pipeline.
- Pressing `o` opens an image list for the current page.
- `Enter` shows the selected image full-screen in the terminal via
  `ratatui-image` (kitty / iTerm2 / sixel, halfblocks fallback); the terminal
  is queried for its protocol lazily on first use (`src/ui/graphics.rs`).
- `o` in the list (or in the viewer) opens the image externally instead:
  it is extracted to a temporary file and handed to:
    1. The user-configured `default_viewer`.
    2. `feh` (if installed).
    3. The system default (`xdg-open`).
- SVG images always use the external viewer (the `image` crate cannot decode them).
- Relative paths for images are resolved against the content document path.

### Page Width Adjustment
- Users can dynamically adjust the text width using `+` and `-`.
- `=` resets the width to the global default: the `width` setting in `configuration.json`, or 80 columns when unset. The same default applies the first time a book is opened.
- The width preference is saved per-book in the database (`reading_states` table).
- Manual adjustments are preserved even when resizing the terminal window.
- Width changes trigger per-chapter re-parsing (cached, very fast) instead of full book re-parsing.

### Cursor & Selection Modes (Text Selection and Yanking)
- Press `v` to enter cursor mode
- Press `v` again to enter selection mode
- Use `hjkl` to move the cursor or extend the selection
- Press `y` to yank (copy) selected text to system clipboard using `arboard`
- `Esc` in selection mode returns to cursor mode; `Esc` in cursor mode returns to reader mode

### Search Implementation
- Press `/` to open search input window
- Supports full regex patterns via the `regex` crate
- Search spans all loaded chapters in the book
- Results highlighted in yellow (configurable in code)
- Navigate with `n` (next), `p` or `N` (previous)
- Matches displayed in context (search window shows line content)
- Current implementation does not distinguish "current hit" visually

### Jump History
- Vim-style jump list navigation
- `Ctrl+o`: jump back to previous position
- `Ctrl+i` or `Tab`: jump forward in history
- Automatically records jumps when: entering search results, following links, navigating to bookmarks, jumping chapters
- History limited to 100 entries (configurable in code)
- Avoids duplicate consecutive entries

### Text-to-Speech (TTS)
- Press `!` to toggle TTS (read aloud) from the current paragraph
- Default file-based engine: `edge-tts` (requires [edge-tts](https://github.com/rany2/edge-tts))
- Also supports `purr`, `trans`, and custom command templates
- Reads sentence-sized chunks; the active chunk is underlined
- Auto-scrolls to keep the current paragraph at the top of the page
- Press `!` again to stop TTS
- Configurable via `preferred_tts_engine` in settings (cycle through presets in Settings window)
- File-based engines are converted in the background with a bounded ready queue so playback can stay ahead
- Temp audio files are deleted after playback ends or TTS is stopped
- Custom engine: set `preferred_tts_engine` to a command template with `{}` for text, or with both `{}` and `{output}` for file-based playback

### Known Limitations
- Supported formats: EPUB, plain text (.txt), Markdown (.md), comic archives (.cbz); MOBI, AZW, FB2 not implemented yet
- Dictionary uses external commands (e.g., `dict`, `sdcv`) and Wikipedia lookup
- No export functionality
- Search does not support: history, fuzzy matching, incremental search
- No custom themes yet (colors hardcoded)
