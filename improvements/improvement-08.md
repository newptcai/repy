# Improvement 08 — TTS reads normalized source text

Status: done

Follow the "Codex Improvement Tasks" section of AGENTS.md for process rules
(tests, checks, commit, status update).

Context: ROADMAP.md Phase 7 item 3, second slice. **Depends on
improvement-07** (reuses the `SourceMap` position-mapping helper). Do not
start before 07 is merged.

## Problem

`build_tts_chunks` (`src/ui/reader/mod.rs:9387`) builds the text sent to the
TTS engine by joining rendered lines with `" "`, then splitting into
sentence chunks (50–100 chars) and computing per-line underline char ranges
from byte offsets into that joined string. Because the input is the rendered
grid, engine input contains justification runs and indent padding
(`RE_TTS_HYPHEN` already patches wrap hyphens, but only those). Spoken
output tolerates extra spaces, but:

- sentence-boundary detection runs on polluted text (indent spaces glue onto
  sentence starts; multi-space runs interact with the 50–100 char budget),
- every engine invocation ships junk whitespace,
- the joined-string offset bookkeeping is a second, TTS-private
  approximation of what `SourceMap` now provides exactly.

## Goal

Chunk text handed to TTS engines is read from `source_map.source_text`
(clean, single-spaced). Underline highlighting on rendered rows behaves
exactly as today from the user's point of view. Paragraph detection, chunk
sizing, auto-scroll, prefetch, and playback control are untouched.

## Requirements

1. **Keep paragraph detection on rendered rows.** The first pass of
   `build_tts_chunks` (blank-row/`CHAPTER_BREAK_MARKER`/`[Image:` scanning
   plus `is_typography_spacing_row`) stays as is — it defines paragraph row
   ranges `(para_start, para_end)`.
2. **Chunk text from source.** For each paragraph row range, resolve its
   source span: `start = offset_for_row(first row)`, `end` = end of the last
   row's span (per-chapter local rows via the existing
   `content_index_for_row` / `content_start_rows` machinery; a paragraph
   never spans chapters — assert or early-split if it somehow does). Take
   `full_text = &source_text[start..end]` and feed that to
   `split_into_sentence_chunks` unchanged. Drop the `RE_TTS_HYPHEN`
   post-processing for this path — source text has no wrap hyphens (leave
   the regex in place only if some other caller still needs it; otherwise
   delete it and its lazy static).
3. **Underline ranges by projection.** Each sentence chunk now has a source
   offset range `[chunk_start, chunk_end)` (chapter-local). Replace the
   joined-string offset table with `SourceMap` projection:
   - rows touched: from `row_for_offset(chunk_start)` through
     `row_for_offset(chunk_end - 1)` (clamp to the paragraph's rows;
     remember to convert to global rows for the `underline` map keys),
   - within each touched row, convert the overlapping source offsets to
     rendered char columns using the inverse of improvement-07's walk. If 07
     shipped only source→offset (`offset_at`), add the inverse
     (`col_at(row, offset, bias)`) next to it in `src/models.rs` with the
     same lockstep rules — rendered-only chars (indent, justification
     padding, wrap hyphen) advance the rendered cursor without consuming
     source.
   - `TtsChunk` keeps its shape (`text`, `first_line`, per-line
     `(col_start, col_end)` char ranges); `first_line` = first touched
     global row.
4. **Equivalence guard.** With justification off and `compact`/default
   paragraph style, chunking must produce the same number of chunks with
   text equal to today's output modulo whitespace (i.e., after collapsing
   whitespace runs in the old output). Encode this as a test, not a hope.

## Tests

Codex writes the tests. Cover at least:

- With justification + `indented` style on a fixture paragraph: chunk text
  contains no double spaces and no leading indent spaces; underline ranges
  land on the same words the chunk speaks (compare against hand-computed
  columns on the rendered lines).
- A sentence spanning a wrap boundary with a hyphenated word: chunk text
  contains the unbroken word; underline covers both partial rows.
- The requirement-4 equivalence test on an unjustified fixture.
- Existing TTS unit tests (`split_into_sentence_chunks`,
  `skip_sentence_trailers`, chunk-boundary tests) keep passing.
- No snapshot changes expected; if underline snapshots exist and change,
  inspect the diff — only exact-boundary off-by-ones from the old byte math
  are acceptable, and only when the new range is the more correct one.

## Out of scope

- Search (improvements 09/10).
- Any change to TTS engines, prefetch queue, playback, auto-scroll, or the
  Settings presets.
- Multi-chapter paragraphs (assert instead).
