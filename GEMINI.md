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
