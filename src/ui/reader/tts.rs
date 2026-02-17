use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::models::{CHAPTER_BREAK_MARKER, MessageType, WindowType};

use super::Reader;

/// A single TTS chunk: the text to speak, the first display line it
/// touches (for scrolling), and the per-line underline column ranges.
pub(super) struct TtsChunk {
    pub text: String,
    pub first_line: usize,
    /// line_num → (start_col, end_col_exclusive) in display characters
    pub underline: HashMap<usize, (usize, usize)>,
}

impl Reader {
    /// Collect text chunks for TTS with precise per-line underline ranges.
    pub(super) fn build_tts_chunks(&self) -> Vec<TtsChunk> {
        let Some(lines) = self.board.lines() else {
            return Vec::new();
        };

        // First pass: collect raw paragraphs as (start, end) line ranges.
        let mut raw_paragraphs: Vec<(usize, usize)> = Vec::new();
        let mut start: Option<usize> = None;
        for (i, line) in lines.iter().enumerate() {
            let is_content =
                !line.is_empty() && line != CHAPTER_BREAK_MARKER && !line.starts_with("[Image:");
            if is_content {
                if start.is_none() {
                    start = Some(i);
                }
            } else if let Some(s) = start.take() {
                raw_paragraphs.push((s, i));
            }
        }
        if let Some(s) = start {
            raw_paragraphs.push((s, lines.len()));
        }

        // Second pass: split each paragraph into sentence-boundary chunks
        // and compute per-line underline character ranges.
        let mut chunks = Vec::new();
        for (para_start, para_end) in raw_paragraphs {
            let para_lines: Vec<&str> = (para_start..para_end)
                .filter_map(|i| lines.get(i).map(String::as_str))
                .collect();
            let full_text = para_lines.join(" ");
            if full_text.trim().is_empty() {
                continue;
            }

            // Build cumulative byte offsets for each line boundary in the
            // joined string.  offsets[i] = byte position where line i starts.
            let mut offsets = Vec::with_capacity(para_lines.len() + 1);
            let mut pos = 0usize;
            for (i, line) in para_lines.iter().enumerate() {
                offsets.push(pos);
                pos += line.len();
                if i + 1 < para_lines.len() {
                    pos += 1; // the " " separator
                }
            }
            offsets.push(pos); // end sentinel

            let sentence_chunks = Self::split_into_sentence_chunks(&full_text, 300, 400);

            let mut byte_cursor = 0usize;
            for chunk_text in sentence_chunks {
                // Advance cursor past inter-chunk whitespace
                while byte_cursor < full_text.len() {
                    if full_text[byte_cursor..].starts_with(chunk_text.as_str()) {
                        break;
                    }
                    byte_cursor += 1;
                }
                let chunk_byte_start = byte_cursor;
                let chunk_byte_end = byte_cursor + chunk_text.len();
                byte_cursor = chunk_byte_end;

                // Compute per-line underline ranges
                let mut underline = HashMap::new();
                let mut first_line = para_start;
                let mut found_first = false;

                for (li, line_text) in para_lines.iter().enumerate() {
                    let line_byte_start = offsets[li];
                    let line_byte_end = line_byte_start + line_text.len();

                    // Check if this line overlaps with the chunk
                    if line_byte_end <= chunk_byte_start || line_byte_start >= chunk_byte_end {
                        continue;
                    }

                    if !found_first {
                        first_line = para_start + li;
                        found_first = true;
                    }

                    // Compute column range within this line (in characters)
                    let overlap_byte_start = chunk_byte_start.max(line_byte_start) - line_byte_start;
                    let overlap_byte_end = chunk_byte_end.min(line_byte_end) - line_byte_start;

                    // Convert byte offsets to character offsets
                    let col_start = line_text[..overlap_byte_start].chars().count();
                    let col_end = line_text[..overlap_byte_end].chars().count();

                    if col_start < col_end {
                        underline.insert(para_start + li, (col_start, col_end));
                    }
                }

                chunks.push(TtsChunk {
                    text: chunk_text,
                    first_line,
                    underline,
                });
            }
        }
        chunks
    }

    /// Check if a period at position `i` in `chars` is a real sentence end.
    /// Filters out abbreviations like "L.", "Mr.", "Dr.", "St.", "e.g.", etc.
    pub(super) fn is_sentence_end(chars: &[char], i: usize) -> bool {
        let ch = chars[i];
        // ? ! ; are almost always sentence endings
        if matches!(ch, '?' | '!' | ';') {
            return i + 1 >= chars.len() || chars[i + 1].is_whitespace();
        }
        if ch != '.' {
            return false;
        }
        // Must be followed by whitespace or end of text
        if i + 1 < chars.len() && !chars[i + 1].is_whitespace() {
            return false;
        }
        // Walk back to find the word before the period
        let mut j = i;
        while j > 0 && chars[j - 1].is_alphabetic() {
            j -= 1;
        }
        let word_len = i - j;
        // Single letter before period → likely an initial (L. , M. , etc.)
        if word_len <= 1 {
            return false;
        }
        // Check for common abbreviations (case-insensitive)
        let word: String = chars[j..i].iter().collect::<String>().to_lowercase();
        let abbrevs = [
            "mr", "mrs", "ms", "dr", "st", "sr", "jr", "prof", "gen", "gov",
            "sgt", "cpl", "pvt", "lt", "col", "maj", "capt", "cmdr", "adm",
            "rev", "hon", "pres", "vs", "etc", "approx", "dept", "est",
            "vol", "fig", "inc", "corp", "ltd", "no",
        ];
        if abbrevs.contains(&word.as_str()) {
            return false;
        }
        true
    }

    /// Split `text` into chunks of approximately `min_len`..`max_len` characters,
    /// breaking at sentence boundaries. Uses `is_sentence_end` for robust detection.
    pub(super) fn split_into_sentence_chunks(text: &str, min_len: usize, max_len: usize) -> Vec<String> {
        let text = text.trim();
        if text.is_empty() {
            return Vec::new();
        }
        if text.len() <= max_len {
            return vec![text.to_string()];
        }

        let chars: Vec<char> = text.chars().collect();
        let mut chunks = Vec::new();
        let mut chunk_start = 0;

        while chunk_start < chars.len() {
            if chars.len() - chunk_start <= max_len {
                let s: String = chars[chunk_start..].iter().collect::<String>().trim().to_string();
                if !s.is_empty() {
                    chunks.push(s);
                }
                break;
            }

            let search_end = (chunk_start + max_len).min(chars.len());
            let search_start = chunk_start + min_len;
            let mut split_at = None;

            // Find the last sentence end in [min_len, max_len]
            for i in search_start..search_end {
                if Self::is_sentence_end(&chars, i) {
                    split_at = Some(i + 1);
                }
            }

            // If none found, scan forward past max_len
            if split_at.is_none() {
                for i in search_end..chars.len() {
                    if Self::is_sentence_end(&chars, i) {
                        split_at = Some(i + 1);
                        break;
                    }
                }
            }

            let end = split_at.unwrap_or(chars.len());
            let chunk: String = chars[chunk_start..end].iter().collect::<String>().trim().to_string();
            if !chunk.is_empty() {
                chunks.push(chunk);
            }
            chunk_start = end;
            while chunk_start < chars.len() && chars[chunk_start].is_whitespace() {
                chunk_start += 1;
            }
        }

        chunks
    }

    /// Find the chunk index whose underline range contains `row`,
    /// or the first chunk starting at or after `row`.
    pub(super) fn find_chunk_at(&self, row: usize) -> Option<usize> {
        for (i, chunk) in self.tts_chunks.iter().enumerate() {
            if chunk.underline.contains_key(&row) {
                return Some(i);
            }
        }
        for (i, chunk) in self.tts_chunks.iter().enumerate() {
            if chunk.first_line >= row {
                return Some(i);
            }
        }
        None
    }

    /// Toggle TTS: start if not active, stop if active.
    pub(super) fn toggle_tts(&mut self) -> eyre::Result<()> {
        if self.state.borrow().ui_state.tts_active {
            self.stop_tts();
            return Ok(());
        }
        self.tts_chunks = self.build_tts_chunks();
        let current_row = self.state.borrow().reading_state.row;
        let idx = match self.find_chunk_at(current_row) {
            Some(i) => i,
            None => {
                let mut state = self.state.borrow_mut();
                state
                    .ui_state
                    .set_message("No text found to read".to_string(), MessageType::Error);
                return Ok(());
            }
        };
        self.tts_chunk_index = idx;
        self.tts_speak_current()?;
        Ok(())
    }

    /// Speak the current chunk.
    pub(super) fn tts_speak_current(&mut self) -> eyre::Result<()> {
        let chunk = match self.tts_chunks.get(self.tts_chunk_index) {
            Some(c) => c,
            None => {
                self.stop_tts();
                return Ok(());
            }
        };

        let text = chunk.text.clone();
        let first_line = chunk.first_line;
        let last_line = chunk
            .underline
            .keys()
            .max()
            .copied()
            .unwrap_or(first_line);
        let underline = chunk.underline.clone();

        // Update UI state: mark active, set underline ranges, scroll
        {
            let mut state = self.state.borrow_mut();
            state.ui_state.tts_active = true;
            state.ui_state.tts_underline_ranges = underline;

            // Smart scroll: only scroll if the chunk isn't fully visible.
            // Scroll just enough so the chunk fits, or until its first line
            // hits the top — whichever comes first.
            let current_top = state.reading_state.row.saturating_sub(1);
            // Compute the real content height: terminal rows minus the
            // layout chrome (top bar + gaps).
            let term_rows = match crossterm::terminal::size() {
                Ok((_, rows)) => rows as usize,
                Err(_) => 24,
            };
            let chrome = if state.config.settings.show_top_bar {
                1 + 2 + 2 // top_bar + top_gap + bottom_gap
            } else {
                2 // bottom_gap only
            };
            let page_height = term_rows.saturating_sub(chrome).max(1);
            let current_bottom = current_top + page_height;

            if first_line >= current_top && last_line < current_bottom {
                // Chunk is entirely visible — don't scroll
            } else {
                // Need to scroll.  Ideal: put last_line at the bottom.
                // new_top = last_line - page_height + 2  (so last_line is the
                // last visible line).  But never scroll past first_line to top.
                let top_to_show_bottom = (last_line + 2).saturating_sub(page_height);
                let new_top = top_to_show_bottom.max(current_top).min(first_line);
                state.reading_state.row = new_top.saturating_add(1);
            }
        }

        // Redraw the screen so the scroll + underline are visible
        // before the TTS process starts speaking.
        {
            let state = self.state.clone();
            self.terminal.draw(|f| {
                let state_ref = state.borrow();
                Self::render_static(f, &state_ref, &self.board, &self.content_start_rows);
            })?;
        }

        // Build command
        let engine = {
            let state = self.state.borrow();
            state
                .config
                .settings
                .preferred_tts_engine
                .clone()
                .unwrap_or_default()
        };

        let (program, args) = if engine.is_empty() || engine == "edge-playback" {
            (
                "edge-playback".to_string(),
                vec!["--text".to_string(), text],
            )
        } else if engine == "espeak" {
            ("espeak".to_string(), vec![text])
        } else if engine == "say" {
            ("say".to_string(), vec![text])
        } else if engine.contains("{}") {
            let expanded = engine.replace("{}", &text);
            let parts: Vec<&str> = expanded.split_whitespace().collect();
            if parts.is_empty() {
                self.stop_tts();
                return Ok(());
            }
            (
                parts[0].to_string(),
                parts[1..].iter().map(|s| s.to_string()).collect(),
            )
        } else {
            (engine, vec![text])
        };

        // Spawn TTS process in its own process group so we can kill all
        // its children (e.g. mpv spawned by edge-playback).
        let (tx, rx) = std::sync::mpsc::channel();
        self.tts_done_rx = Some(rx);

        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }

        match cmd.spawn() {
            Ok(child) => {
                let pid = child.id();
                let mut child_for_thread = child;
                std::thread::spawn(move || {
                    let _ = child_for_thread.wait();
                    let _ = tx.send(());
                });
                self.tts_child = None;
                self.tts_kill_pid = Some(pid);
            }
            Err(err) => {
                self.stop_tts();
                let mut state = self.state.borrow_mut();
                state.ui_state.set_message(
                    format!("TTS failed: {err}"),
                    MessageType::Error,
                );
            }
        }

        Ok(())
    }

    /// Advance to the next chunk after the current one finishes.
    pub(super) fn tts_advance_paragraph(&mut self) -> eyre::Result<()> {
        self.tts_chunk_index += 1;
        if self.tts_chunk_index >= self.tts_chunks.len() {
            self.stop_tts();
            let mut state = self.state.borrow_mut();
            state
                .ui_state
                .set_message("TTS finished".to_string(), MessageType::Info);
            return Ok(());
        }
        self.tts_speak_current()
    }

    /// Stop TTS playback — kill the entire process group.
    pub(super) fn stop_tts(&mut self) {
        if let Some(pid) = self.tts_kill_pid.take() {
            #[cfg(unix)]
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            #[cfg(not(unix))]
            {
                if let Some(mut child) = self.tts_child.take() {
                    let _ = child.kill();
                }
            }
        }
        if let Some(mut child) = self.tts_child.take() {
            let _ = child.kill();
        }
        self.tts_done_rx = None;
        self.tts_chunks.clear();
        self.tts_chunk_index = 0;
        let mut state = self.state.borrow_mut();
        state.ui_state.tts_active = false;
        state.ui_state.tts_underline_ranges.clear();
    }
}
