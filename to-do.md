# Comprehensive Rust Porting Plan: epy to repy

Concise roadmap for the Rust port of `epy`.

## Current Progress (January 2026)

### Completed Features ✅

**Core Infrastructure:**
- Complete config system with XDG support (`src/config.rs`)
- SQLite-backed state management with bundled SQLite (`src/state.rs`)
- EPUB parsing with HTML-to-text conversion (`src/ebook.rs`, `src/parser.rs`)
- Hyphenation and smart text wrapping with textwrap
- Per-chapter text structure caching for fast rendering

**Reading Experience:**
- Full terminal UI with ratatui (`src/ui/reader.rs`, `src/ui/board.rs`)
- All navigation modes: line, page, half-page, chapter, book-level
- Jump history navigation (Ctrl+o / Ctrl+i / Tab)
- Visual mode for text selection and yanking to clipboard
- Regex search with highlighting (/, n, p/N)
- Width adjustment (+/- keys, = to reset) with per-book persistence
- Line numbers toggle (in settings window)
- Top bar toggle (T key)

**Windows & Features:**
- Help window with full keybinding reference (`src/ui/windows/help.rs`)
- Table of Contents (`src/ui/windows/toc.rs`)
- Bookmarks with add/delete/jump (`src/ui/windows/bookmarks.rs`)
- Library/History with deletion (`src/ui/windows/library.rs`)
- Search window (`src/ui/windows/search.rs`)
- Links window with contextual preview (`src/ui/windows/links.rs`)
- Images window with extraction and viewing (`src/ui/windows/images.rs`)
- Metadata viewer (`src/ui/windows/metadata.rs`)
- Settings window (`src/ui/windows/settings.rs`)

**State Persistence:**
- Reading position saved per-book (chapter, row, percentage)
- Width preferences saved per-book
- Auto-resume last book on startup without arguments
- Library metadata with reading progress tracking
- Named bookmarks stored in database

**Testing:**
- Comprehensive test suite covering parser, footnotes, images, jump history, hyphenation

### Not Yet Implemented ❌

**Layout & UI:**
- Bottom status bar for command echo and transient messages
- 'b' and 'f' keybindings (documented in README but not implemented)

**Advanced Search:**
- Search history and saved searches
- Fuzzy search and typo tolerance
- Incremental search with real-time results

**External Integrations:**
- Dictionary integration (sdcv, dict, etc.)
- Export functionality (text, highlights)

**Text-to-Speech:**
- TTS trait system
- GTTS + MPV engine
- Mimic/Pico engines
- Voice selection, speed control, pronunciation dictionaries

**Advanced Features:**
- Additional format support (MOBI, AZW, FB2)
- Reading statistics and analytics
- Annotation system beyond bookmarks
- Custom themes and color schemes

**Distribution & Polish:**
- CI/CD pipeline
- Platform packages (deb, wix, etc.)
- Property-based testing
- Cross-platform compatibility testing
- Migration guide from epy

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
    *   [x] Header bar (Implemented with title and status)
    *   [x] Minimal chrome
    *   [ ] Bottom Status Bar (Command echo, transient messages)
    *   [x] Margins/padding (Automatic centering and dynamic width adjustment implemented)
    *   [x] Image placeholder styling (Centered and descriptive)
    *   [x] Line numbers toggle
    *   [x] Help window parity
    *   [x] Inline bold/italic rendering (strip markdown markers)
    *   [x] Hyphenation and smart wrapping
    *   [x] Navigation History (Ctrl+o / Ctrl+i)

10. **Text-to-Speech Integration (`src/tts/`):**
    *   [ ] Create TTS trait system for multiple engine support
    *   [ ] **GTTS + MPV Engine**: Async TTS with progress tracking
    *   [ ] **Mimic Engine**: Local TTS synthesis integration
    *   [ ] **Pico Engine**: Cross-platform voice synthesis
    *   [ ] Add voice selection, speed control, and pronunciation dictionaries
    *   [ ] Implement reading position synchronization with TTS

11. **Advanced Search (`src/search.rs`):**
    *   [x] Multi-chapter regex search (Search iterates over full loaded book content)
    *   [x] Search result highlighting with configurable colors
    *   [ ] Search history and saved searches
    *   [ ] Fuzzy search and typo tolerance
    *   [ ] Incremental search with real-time results

12. **External Tool Integration (`src/tools/`):**
    *   [ ] **Dictionary Integration**: Multiple dictionary engines (sdcv, dict, etc.)
    *   [x] **Image Viewer Integration**: List images on page, extract and open with system viewer
    *   [x] **URL Handling**: Internal anchor jumps, footnotes, and external link opening
    *   [x] **Link Preview**: Contextual preview of internal links/footnotes in the links window
    *   [x] **Footnote Formatting**: User-friendly display of footnote labels (e.g., "Footnote 2")
    *   [ ] **Export Functionality**: Text and highlighted content export

13. **Utilities (`src/utils.rs`):**
    *   [ ] Port the helper functions from `epy/src/epy_reader/utils.py` and `epy/src/epy_reader/lib.py` to a `utils` module.
    *   [ ] Add platform-specific utilities (Windows/Linux/macOS)
    *   [ ] Implement file format detection and validation
    *   [x] Add logging and debugging utilities (Basic implementation in `src/logging.rs`)

### Phase 4: Performance & Polish (PENDING ⏳)

14. **Performance Optimization:**
    *   [x] Per-chapter text structure caching for fast padding/width adjustments (O(1) instead of O(n))
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
    *   [x] Comprehensive test suite with integration tests
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
