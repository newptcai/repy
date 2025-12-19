# Comprehensive Rust Porting Plan: epy to repy

A concise plan for porting the Python-based epub reader `epy` to the Rust-based `repy`.

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
- Text-to-speech integration
- Advanced search
- External tool integration
- Utilities

### Phase 4: Performance & Polish (PENDING ⏳)
- Performance optimization
- Advanced features
- Quality assurance

### Phase 5: Integration & Deployment (PENDING ⏳)
- Integration
- Distribution & documentation

## Layout Parity TODOs (epy vs repy)
- Header bar
- Minimal chrome
- Footer/status
- Margins/padding
- Image placeholder styling
- Line numbers toggle
- Help window parity

## Development Guidelines
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
