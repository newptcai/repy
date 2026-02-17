use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use textwrap::{Options, WordSplitter};

pub struct DictionaryWindow;

/// A logical paragraph: its leading indent (in columns) and the joined text content.
struct LogicalParagraph {
    indent: usize,
    text: String,
}

impl DictionaryWindow {
    pub fn max_scroll_offset(area: Rect, definition: &str, loading: bool) -> u16 {
        if loading {
            return 0;
        }

        let popup_area = super::centered_popup_area(area, 70, 80);
        let inner_width = popup_area.width.saturating_sub(2) as usize;
        let inner_height = popup_area.height.saturating_sub(2) as usize;
        let reflowed = Self::reflow(definition, inner_width);
        let total_lines = reflowed.lines().count();

        total_lines
            .saturating_sub(inner_height)
            .min(u16::MAX as usize) as u16
    }

    pub fn render(
        frame: &mut Frame,
        area: Rect,
        word: &str,
        definition: &str,
        scroll_offset: u16,
        loading: bool,
        is_wikipedia: bool,
    ) {
        let popup_area = super::centered_popup_area(area, 70, 80);
        frame.render_widget(Clear, popup_area);

        let label = if is_wikipedia { "Wikipedia" } else { "Dictionary" };
        let title = if word.is_empty() {
            label.to_string()
        } else {
            format!("{label}: {word}")
        };

        let block = Block::default().title(title).borders(Borders::ALL);

        if loading {
            let loading_text = vec![
                "⠋ Loading...",
                "⠙ Loading...",
                "⠹ Loading...",
                "⠸ Loading...",
                "⠼ Loading...",
                "⠴ Loading...",
                "⠦ Loading...",
                "⠧ Loading...",
                "⠇ Loading...",
                "⠏ Loading...",
            ];
            // Use time to select the spinner frame
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let frame_idx = (now / 100) as usize % loading_text.len();
            let paragraph = Paragraph::new(loading_text[frame_idx]).block(block);
            frame.render_widget(paragraph, popup_area);
            return;
        }

        // Inner width = popup width minus 2 for borders
        let inner_width = popup_area.width.saturating_sub(2) as usize;
        let reflowed = Self::reflow(definition, inner_width);

        let paragraph = Paragraph::new(reflowed)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((scroll_offset, 0));

        frame.render_widget(paragraph, popup_area);
    }

    /// Count leading whitespace columns (treating each char as 1 column).
    fn indent_of(line: &str) -> usize {
        line.len() - line.trim_start().len()
    }

    /// Join continuation lines into logical paragraphs.
    ///
    /// Dictionary programs (dict, sdcv, wkdict) wrap their output at ~80 columns
    /// and indent continuation lines deeper than the entry start. For example:
    ///
    /// ```text
    ///       n 1: a feeling of extreme pleasure or satisfaction; "his delight
    ///            to see her was obvious to all" [syn: {delight}, {delectation}]
    ///       2: something or someone that provides a source of happiness; "a
    ///          joy to behold"
    /// ```
    ///
    /// Here `n 1:` starts at indent 6 and its continuation is at indent 11.
    /// Entry `2:` also starts at indent 6 — that's a new paragraph.
    ///
    /// A deeper-indented line is only treated as continuation when the previous
    /// raw line was long enough to have been wrapped (near the max line length),
    /// or we are already inside a continuation sequence. This prevents short
    /// headings (like `  emperor`) from swallowing their sub-entries.
    fn join_continuations(text: &str) -> Vec<LogicalParagraph> {
        // Approximate the wrapping width the dictionary program used.
        // Lines close to this length were likely wrapped mid-sentence.
        let max_line_len = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len())
            .max()
            .unwrap_or(80);
        let wrap_threshold = max_line_len.saturating_sub(15).max(40);

        let mut paragraphs: Vec<LogicalParagraph> = Vec::new();
        let mut prev_raw_len: usize = 0;
        let mut in_continuation = false;

        for line in text.lines() {
            if line.trim().is_empty() {
                paragraphs.push(LogicalParagraph {
                    indent: 0,
                    text: String::new(),
                });
                prev_raw_len = 0;
                in_continuation = false;
                continue;
            }

            let indent = Self::indent_of(line);
            let content = line.trim();

            let is_continuation = if let Some(prev) = paragraphs.last() {
                // Continuation requires:
                // 1. Previous paragraph is non-blank
                // 2. Current indent is strictly deeper than the paragraph base
                // 3. Either the previous raw line was long (likely wrapped) or
                //    we are already inside a continuation sequence
                !prev.text.is_empty()
                    && indent > prev.indent
                    && (prev_raw_len >= wrap_threshold || in_continuation)
            } else {
                false
            };

            if is_continuation {
                in_continuation = true;
                let prev = paragraphs.last_mut().unwrap();
                prev.text.push(' ');
                prev.text.push_str(content);
            } else {
                in_continuation = false;
                paragraphs.push(LogicalParagraph {
                    indent,
                    text: content.to_string(),
                });
            }

            prev_raw_len = line.len();
        }

        paragraphs
    }

    /// Reflow dictionary output to fit within `width` columns.
    ///
    /// 1. Join continuation lines into logical paragraphs (undo the original
    ///    80-column wrapping from the dictionary program).
    /// 2. Re-wrap each paragraph to the target width, preserving original
    ///    indentation.
    fn reflow(text: &str, width: usize) -> String {
        if width == 0 {
            return text.to_string();
        }

        let paragraphs = Self::join_continuations(text);
        let mut result = Vec::new();

        for para in &paragraphs {
            if para.text.is_empty() {
                result.push(String::new());
                continue;
            }

            let indent: String = " ".repeat(para.indent);
            let available = width.saturating_sub(para.indent);

            if available < 10 || para.text.len() + para.indent <= width {
                // Fits on one line or too narrow to wrap — emit as-is
                result.push(format!("{indent}{}", para.text));
            } else {
                let options = Options::new(available).word_splitter(WordSplitter::NoHyphenation);
                let wrapped = textwrap::wrap(&para.text, &options);
                for w in wrapped {
                    result.push(format!("{indent}{}", w.trim_end()));
                }
            }
        }

        result.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflow_preserves_short_lines() {
        let input = "hello\nworld";
        assert_eq!(DictionaryWindow::reflow(input, 40), "hello\nworld");
    }

    #[test]
    fn reflow_preserves_blank_lines() {
        let input = "para one\n\npara two";
        let result = DictionaryWindow::reflow(input, 80);
        assert_eq!(result, "para one\n\npara two");
    }

    #[test]
    fn reflow_preserves_indentation() {
        let input = "  indented line that is short";
        let result = DictionaryWindow::reflow(input, 80);
        assert_eq!(result, "  indented line that is short");
    }

    #[test]
    fn reflow_wraps_indented_long_line() {
        let input = "    this is a long indented line that needs to be wrapped to a narrower width for display";
        let result = DictionaryWindow::reflow(input, 40);
        for line in result.lines() {
            assert!(
                line.len() <= 40,
                "line too long: {:?} ({})",
                line,
                line.len()
            );
            assert!(line.starts_with("    "), "indent lost: {:?}", line);
        }
    }

    #[test]
    fn join_continuations_merges_deeper_indented_lines() {
        // Simulates WordNet-style output where continuation is indented deeper.
        // Lines must be long enough (~70 chars) to look like they were wrapped.
        let input = concat!(
            "      n 1: a feeling of extreme pleasure or satisfaction; \"his delight\n",
            "           to see her was obvious to all\" [syn: {delight}]\n",
            "      2: something or someone that provides a source of happiness; \"a\n",
            "         joy to behold\"",
        );
        let paras = DictionaryWindow::join_continuations(input);
        assert_eq!(paras.len(), 2);
        assert_eq!(paras[0].indent, 6);
        assert!(paras[0].text.starts_with("n 1:"));
        assert!(paras[0].text.contains("to see her"));
        assert_eq!(paras[1].indent, 6);
        assert!(paras[1].text.starts_with("2:"));
        assert!(paras[1].text.contains("joy to behold"));
    }

    #[test]
    fn join_continuations_preserves_blank_separators() {
        let input = "header\n\n  entry";
        let paras = DictionaryWindow::join_continuations(input);
        assert_eq!(paras.len(), 3);
        assert_eq!(paras[0].text, "header");
        assert!(paras[1].text.is_empty()); // blank separator
        assert_eq!(paras[2].text, "entry");
    }

    #[test]
    fn join_continuations_short_heading_not_merged() {
        // A short heading like "  emperor" should NOT swallow deeper-indented
        // sub-entries — the heading wasn't wrapped, so deeper lines are
        // structurally distinct entries.
        let input = concat!(
            "  emperor\n",
            "      n 1: the male ruler of an empire\n",
            "      2: red table grape of California\n",
        );
        let paras = DictionaryWindow::join_continuations(input);
        assert_eq!(paras.len(), 3);
        assert_eq!(paras[0].text, "emperor");
        assert_eq!(paras[0].indent, 2);
        assert!(paras[1].text.starts_with("n 1:"));
        assert_eq!(paras[1].indent, 6);
        assert!(paras[2].text.starts_with("2:"));
        assert_eq!(paras[2].indent, 6);
    }

    #[test]
    fn reflow_wordnet_entry() {
        // Full WordNet-style output: short heading + wrapped definitions
        let input = concat!(
            "From WordNet (r) 3.0 (2006) [wn]:\n",
            "\n",
            "  delight\n",
            "      n 1: a feeling of extreme pleasure or satisfaction; \"his delight\n",
            "           to see her was obvious to all\" [syn: {delight},\n",
            "           {delectation}]\n",
            "      2: something or someone that provides a source of\n",
            "         happiness; \"a joy to behold\" [syn: {joy}, {delight}]\n",
            "      v 1: give pleasure to or be pleasing to; \"These colors please\n",
            "           the senses\" [syn: {please}, {delight}] [ant: {displease}]\n",
        );
        let result = DictionaryWindow::reflow(input, 50);
        for line in result.lines() {
            assert!(
                line.len() <= 50,
                "line too long: {:?} ({})",
                line,
                line.len()
            );
        }
        // "delight" heading should be on its own line, not merged with n 1:
        assert!(result.contains("\n  delight\n"));
        // The joined "n 1:" entry should contain the full text
        assert!(result.contains("satisfaction;"));
        assert!(result.contains("{delectation}]"));
    }

    #[test]
    fn reflow_same_indent_stays_separate() {
        // Two lines at the same indent should NOT be joined
        let input = "  line one\n  line two";
        let result = DictionaryWindow::reflow(input, 80);
        assert_eq!(result, "  line one\n  line two");
    }

    #[test]
    fn reflow_emperor_wordnet_section() {
        // The WordNet section for "emperor" should keep each definition separate
        let input = concat!(
            "  emperor\n",
            "      n 1: the male ruler of an empire\n",
            "      2: red table grape of California\n",
            "      3: large moth of temperate forests of Eurasia having heavily scaled\n",
            "         transparent wings [syn: {emperor}, {emperor moth}]\n",
            "      4: large richly colored butterfly [syn: {emperor butterfly},\n",
            "         {emperor}]\n",
        );
        let result = DictionaryWindow::reflow(input, 55);
        // "emperor" heading must not be merged with definitions
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines[0].trim(), "emperor");
        // Each "n 1:", "2:", "3:", "4:" should start a separate paragraph
        let def_starts: Vec<&str> = lines
            .iter()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("n 1:")
                    || t.starts_with("2:")
                    || t.starts_with("3:")
                    || t.starts_with("4:")
            })
            .copied()
            .collect();
        assert_eq!(
            def_starts.len(),
            4,
            "expected 4 definition starts, got: {def_starts:?}"
        );
        // Definition 3 continuation should be merged (not a separate line starting with "transparent")
        let has_transparent_start = lines.iter().any(|l| l.trim().starts_with("transparent"));
        assert!(
            !has_transparent_start,
            "continuation should be merged into definition 3"
        );
    }

    #[test]
    fn max_scroll_offset_zero_when_content_fits() {
        let area = Rect::new(0, 0, 120, 40);
        let definition = "short line";
        assert_eq!(
            DictionaryWindow::max_scroll_offset(area, definition, false),
            0
        );
    }

    #[test]
    fn max_scroll_offset_positive_when_content_overflows() {
        let area = Rect::new(0, 0, 120, 40);
        let definition = (0..80).map(|_| "line").collect::<Vec<_>>().join("\n");
        let max = DictionaryWindow::max_scroll_offset(area, &definition, false);
        assert!(max > 0);
    }
}
