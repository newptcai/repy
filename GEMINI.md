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
- Commit frequently with small, focused changes.
- Test-driven development for complex components.
- Cross-platform mindset (Linux/macOS/Windows).
- Preserve epy behavior while improving performance.


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
├── ebook.rs             # EPUB parsing and book data structures
├── parser.rs            # HTML-to-text conversion, wrapping, hyphenation
├── models.rs            # Data models (ReadingState, SearchData, WindowType, etc.)
├── settings.rs          # User settings structure
├── logging.rs           # Debug logging utilities
├── ui/
│   ├── mod.rs           # UI module root
│   ├── reader/
│   │   ├── mod.rs       # Main reader application state and event handling
│   │   └── tts.rs       # Text-to-speech engine abstraction
│   ├── board.rs         # Rendering logic for the reading view
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

**`src/ebook.rs`** (~700 lines):
- EPUB parsing using the `epub` crate
- Chapter extraction and content indexing
- Metadata extraction (title, author, etc.)

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

**`src/ui/reader/tts.rs`** (~450 lines):
- TTS engine abstraction (TtsEngine trait)
- Preset engine support: edge-playback, espeak, say (macOS)
- Custom engine templates with `{}` placeholder
- Paragraph-by-paragraph reading with async child process management

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
- Current coverage: 162 test cases across unit and integration tests

### Dependencies
- `ratatui` 0.30.0-beta.0: Terminal UI framework
- `crossterm` 0.29.0: Cross-platform terminal manipulation
- `epub` 2.1.5: EPUB file parsing
- `rusqlite` 0.37.0 (bundled): SQLite database
- `scraper` 0.24.0: HTML parsing
- `hyphenation` 0.8.4 (embed_en-us): Hyphenation engine
- `textwrap` 0.16.1: Text wrapping with hyphenation support
- `clap` 4.5.53: CLI argument parsing
- `arboard` 3.6.1: Clipboard access for yank functionality

## Feature Details

### Image Handling
- Images are preprocessed to include descriptive alt text (e.g., `[Image: filename.jpg]`).
- Image placeholders are centered in the reader view.
- Pressing `o` opens an image list for the current page.
- Selecting an image extracts it to a temporary file.
- The viewer attempts to open the image using:
    1. The user-configured `default_viewer`.
    2. `feh` (if installed).
    3. The system default (`xdg-open`).
- Relative paths for images are resolved against the content document path.

### Page Width Adjustment
- Users can dynamically adjust the text width using `+` and `-`.
- `=` resets the width to the global default (default 80 columns).
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
- Default engine: `edge-playback` (from `edge-tts` Python package, uses Microsoft Edge as backend)
- Reads paragraph by paragraph; the active paragraph is underlined
- Auto-scrolls to keep the current paragraph at the top of the page
- Press `Esc` or `!` again to stop TTS
- Configurable via `preferred_tts_engine` in settings (cycle through presets in Settings window)
- Preset engines: `edge-playback`, `espeak`, `say` (macOS)
- Custom engine: set `preferred_tts_engine` to a command template with `{}` placeholder for text (e.g., `espeak -s 150 "{}"`)

### Known Limitations
- Only EPUB format supported (MOBI, AZW, FB2 not implemented)
- Dictionary uses external commands (e.g., `dict`, `sdcv`) and Wikipedia lookup
- No export functionality
- No bottom status bar (command echo, messages)
- Keybindings `b` and `f` documented in README but not implemented in code
- Search does not support: history, fuzzy matching, incremental search
- No custom themes yet (colors hardcoded)
