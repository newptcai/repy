# Comprehensive Rust Porting Plan: epy to repy

Concise roadmap for the Rust port of `epy`.

## Current Progress (Dec 2025)

- Core configuration and app data prefix handling are implemented (`src/config.rs`).
- SQLite-backed reading state, library/history, and bookmarks are implemented (`src/state.rs`).
- In-TUI library/history window with selection and deletion is wired into the reader (`src/ui/reader.rs`, `src/ui/windows/library.rs`).
- On quit, the current book position and progress are persisted; on startup with no arguments, the last-read book is reopened if available (`src/ui/reader.rs`, `src/main.rs`).
- Jump history navigation (Ctrl+o / Ctrl+i) is implemented.
- Footnote handling is robust (correct jumping and backlink filtering).
- SQLite is fully managed from Rust with `rusqlite`’s `bundled` feature, so no system `libsqlite3` is required (`Cargo.toml`).
- A user-facing `README.md` documents configuration, database paths, and basic usage.

What is *not* done yet (high level):

- Layout parity polish (header/footer chrome, margins, image placeholders, line numbers toggle, help window parity).
- Advanced search features (multi-chapter regex, search history, fuzzy search, incremental search).
- Text-to-speech trait system and engines.
- External tool integration (dictionary, image viewer, export).
- Performance work and packaging/CI.

The detailed roadmap below remains the source of truth for planned work.

## Roadmap

### Phase 1: Core Infrastructure (COMPLETED ✅)
- Project setup
- Basic structure & error handling
- Data models
- Configuration
- Application state
- Ebook parsing

### Phase 2: Terminal UI Infrastructure (COMPLETED ✅)
- Terminal UI
- Command-line interface

### Phase 3: Advanced Features (PENDING ⏳)

9.  **Layout Parity (epy vs repy):**
    *   [ ] Header bar
    *   [ ] Minimal chrome
    *   [ ] Footer/status
    *   [ ] Margins/padding
    *   [ ] Image placeholder styling
    *   [ ] Line numbers toggle
    *   [ ] Help window parity
    *   [x] Inline bold/italic rendering (strip markdown markers)
    *   [x] Navigation History (Ctrl+o / Ctrl+i)

10. **Text-to-Speech Integration (`src/tts/`):**
    *   [ ] Create TTS trait system for multiple engine support
    *   [ ] **GTTS + MPV Engine**: Async TTS with progress tracking
    *   [ ] **Mimic Engine**: Local TTS synthesis integration
    *   [ ] **Pico Engine**: Cross-platform voice synthesis
    *   [ ] Add voice selection, speed control, and pronunciation dictionaries
    *   [ ] Implement reading position synchronization with TTS

11. **Advanced Search (`src/search.rs`):**
    *   [ ] Multi-chapter regex search with performance optimization
    *   [ ] Search result highlighting with configurable colors
    *   [ ] Search history and saved searches
    *   [ ] Fuzzy search and typo tolerance
    *   [ ] Incremental search with real-time results

12. **External Tool Integration (`src/tools/`):**
    *   [ ] **Dictionary Integration**: Multiple dictionary engines (sdcv, dict, etc.)
    *   [ ] **Image Viewer Integration**: Cross-platform image display
    *   [x] **URL Handling**: Internal anchor jumps, footnotes, and external link opening
    *   [ ] **Export Functionality**: Text and highlighted content export

13. **Utilities (`src/utils.rs`):**
    *   [ ] Port the helper functions from `epy/src/epy_reader/utils.py` and `epy/src/epy_reader/lib.py` to a `utils` module.
    *   [ ] Add platform-specific utilities (Windows/Linux/macOS)
    *   [ ] Implement file format detection and validation
    *   [ ] Add logging and debugging utilities

### Phase 4: Performance & Polish (PENDING ⏳)

14. **Performance Optimization:**
    *   [ ] Implement async book loading and caching
    *   [ ] Optimize large book handling with lazy loading
    *   [ ] Add memory management for massive texts
    *   [ ] Performance profiling and benchmarking

15. **Advanced Features:**
    *   [ ] **Multiple Format Support**: MOBI, AZW, FB2 parsers using trait system
    *   [ ] **Reading Statistics**: Track reading speed, habits, and progress
    *   [ ] **Annotation System**: Marginal notes and highlighting
    *   [ ] **Custom Themes**: User-defined color schemes and layouts

16. **Quality Assurance:**
    *   [ ] Comprehensive test suite with integration tests
    *   [ ] Property-based testing for critical components
    *   [ ] Performance testing and memory profiling
    *   [ ] Cross-platform compatibility testing
    *   [ ] Accessibility features and usability testing

### Phase 5: Integration & Deployment (PENDING ⏳)

17. **Integration (`src/main.rs`):**
    *   [x] Tie all the modules together in the `main` function.
    *   [x] Initialize the configuration and state.
    *   [x] Parse command-line arguments.
    *   [x] Set up the terminal for `ratatui`.
    *   [x] Create and run the main `Reader` application.
    *   [x] Ensure graceful shutdown and terminal restoration.

18. **Distribution & Documentation:**
    *   [ ] Create build scripts and CI/CD pipeline
    *   [ ] Package for multiple platforms (cargo-deb, cargo-wix, etc.)
    *   [ ] Comprehensive user documentation and README
    *   [ ] Developer documentation and contribution guidelines
    *   [ ] Migration guide from epy to repy

## Development Guidelines
- AGENTS.md, GEMINI.md, and CLAUDE.md are the same file; changing one updates the other two automatically.
- Commit frequently with small, focused changes.
- Test-driven development for complex components.
- Cross-platform mindset (Linux/macOS/Windows).
- Preserve epy behavior while improving performance.
- Initialize the `epy` submodule to consult original code; if SSH access is unavailable, switch the submodule URL to HTTPS before running `git submodule update --init --recursive`.

## Success Metrics
- Feature parity
- Performance improvement
- Memory efficiency
- Reliability
- Maintainability
