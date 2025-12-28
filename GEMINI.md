## Development Guidelines
- **Symlinks**: `AGENTS.md`, `GEMINI.md`, and `CLAUDE.md` are linked with symbolic links. Changing one is enough.
- **Commits**: Commit frequently with small, focused changes.
- **Testing**: Test-driven development for complex components. Always run `cargo test` after adding a new feature or fixing a bug to ensure code quality and prevent regressions.
- **Cross-platform**: Maintain support for Linux, macOS, and Windows.
- **Parity**: Preserve `epy` behavior while improving performance and code safety.
- **Reference**: Initialize the `epy` submodule (`git submodule update --init --recursive`) to consult original Python code.

## Architecture & Codebase Map
`repy` is structured into modular components:

- **Entry Point**: `src/main.rs` initializes logging, config, and starts the `Reader` TUI.
- **Core Models**: `src/models.rs` defines shared data structures (`BookMetadata`, `ReadingState`, `TextStructure`, `TocEntry`).
- **Configuration**: `src/config.rs` handles `configuration.json` loading/saving and defaults.
- **State Management**: `src/state.rs` manages the SQLite database (`states.db`) for library history, reading progress, and bookmarks.
- **Ebook Handling**: `src/ebook.rs` defines the `Ebook` trait and `Epub` implementation using the `epub` crate.
- **Parsing**: `src/parser.rs` converts HTML content to text using `html2text` and `scraper`, extracting structure (images, links, formatting).
- **UI (TUI)**: `src/ui/` powered by `ratatui` and `crossterm`.
    - `reader.rs`: The main application loop and event handler. Manages `ApplicationState` and `UiState`.
    - `board.rs`: Renders the main text content with formatting and line wrapping.
    - `windows/`: Modal dialogs (Help, TOC, Library, etc.).

## Key Technologies
- **TUI**: `ratatui`, `crossterm`
- **Data**: `rusqlite` (bundled), `serde_json`
- **Parsing**: `epub`, `html2text`, `scraper`, `regex`
- **Utilities**: `eyre` (errors), `clap` (CLI), `arboard` (clipboard)

## Future Development & Design Principles
- **Widget Purity**: Keep UI widgets in `src/ui/windows/` stateless. They should take data via parameters rather than managing their own state.
- **Extensibility**:
    - Add new ebook formats (MOBI, AZW3, FB2) by implementing the `Ebook` trait in `src/ebook.rs`.
    - Implement TTS by creating a `TtsEngine` trait and implementing it for different backends (gTTS, Mimic, Pico).
- **Asynchronous Operations**: For heavy tasks like loading large books or counting letters, consider using background threads or async tasks to keep the UI responsive.
- **Search Optimization**: Multi-chapter search should be efficient. Consider pre-indexing or incremental loading for very large books.
- **Styling**: Move towards using `ratatui::style::Style` instead of raw `u32` attributes in `InlineStyle` for better TUI integration.

## Current Status (Dec 2025)
See `roadmap.md` for detailed tracking.
- ✅ Core Infrastructure (Config, DB, Models)
- ✅ Basic EPUB reading & rendering
- ✅ Navigation (Chapters, TOC, Jump History, Links)
- ✅ Library & Bookmarks
- ⏳ Advanced Search (Regex implemented, needs multi-chapter polish)
- ⏳ Text-to-Speech (Trait system and engines pending)
- ⏳ Image Viewing (Integration with external viewers pending)

## Success Metrics
- Feature parity with `epy`
- Performance improvement (startup time, rendering speed)
- Memory efficiency
- Reliability (robust error handling, safe Rust)
- Maintainability