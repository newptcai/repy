---
name: verify
description: Drive the repy TUI end-to-end in an isolated tmux session to verify reader changes at the real terminal surface.
---

# Verifying repy changes

repy is a TUI; verify changes by driving the real binary in tmux, not by
re-running tests.

## Launch (isolated: keeps the real states.db untouched)

```bash
cargo build
SCRATCH=$(mktemp -d)
tmux -L repy-verify kill-server 2>/dev/null
tmux -L repy-verify new-session -d -x 100 -y 30 \
  "env HOME=$SCRATCH XDG_CONFIG_HOME=$SCRATCH/.config XDG_DATA_HOME=$SCRATCH/.local/share \
   ./target/debug/repy tests/fixtures/small.epub"
```

## Drive and capture

```bash
tmux -L repy-verify send-keys o        # key names: Enter, Escape, or literal chars
tmux -L repy-verify capture-pane -p    # the evidence
tmux -L repy-verify resize-window -x 60 -y 20   # resize probe
tmux -L repy-verify kill-server        # cleanup
```

## Gotchas

- **Sleep ≥2s after keys that do real work.** The first in-terminal image
  open blocks ~2s on the terminal graphics query (ratatui-image stdio
  probe times out inside tmux), and debug-build JPEG decode takes ~1s.
  Captures taken too early race the redraw and mislead.
- Don't send Escape immediately followed by another key — crossterm can
  parse the pair as Alt+<key>. Sleep between them.
- Keys sent while the one-time graphics stdio probe is running are
  swallowed (ratatui-image's response reader consumes stdin). Wait for the
  first image/cover to actually appear before sending navigation keys, or
  captures will show a state that doesn't match the keys you sent.
- Don't press `o` in the images list / image viewer during verification
  unless you want feh popping up on the user's desktop; `open_image_viewer`
  also blocks the TUI until the external viewer exits.
- `tests/fixtures/small.epub` page 1 shows a cover-image placeholder —
  handy for image-flow verification (`o` → images list).
- In tmux the graphics query falls back to halfblocks, so in-terminal
  images render as ▀▄ block characters; kitty/sixel need a real terminal
  (manual matrix per ROADMAP).
