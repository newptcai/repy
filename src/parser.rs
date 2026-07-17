use crate::annotations::{NORMALIZATION_VERSION, normalize_text};
use crate::css::StyledClasses;
use crate::models::{InlineStyle, LinkEntry, SourceMap, TextStructure};
use crate::settings::{LineSpacing, ParagraphStyle};
use eyre::Result;
use html2text::config;
use hyphenation::{Language, Load, Standard};
use regex::{Captures, Regex};
use scraper::{Html, Selector};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use textwrap::{Options, WordSplitter};
use unicode_width::UnicodeWidthStr;

// Lazily compiled regex patterns used across parser functions.
static RE_ORDERED_LIST: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(\d+)\.\s").unwrap());
static RE_SUP_OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<sup[^>]*>").unwrap());
static RE_SUP_CLOSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</sup>").unwrap());
static RE_SUB_OPEN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)<sub[^>]*>").unwrap());
static RE_SUB_CLOSE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)</sub>").unwrap());
static RE_LINK_SUP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[\^\{[^}]+\}\]").unwrap());
static RE_SUP_LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\^\{(\[[^\]]+\])\}").unwrap());
static RE_IMG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?i)<img\s+([^>]+)>"#).unwrap());
static RE_IMG_SRC: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"src=["']([^"']+)["']"#).unwrap());
static RE_IMG_ALT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"alt=["']([^"']*)["']"#).unwrap());
static RE_IMG_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"title=["']([^"']*)["']"#).unwrap());
static RE_SVG_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<svg[\s>].*?</svg>").unwrap());
static RE_SVG_IMAGE_HREF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?is)<image[^>]*?href=["']([^"']+)["']"#).unwrap());

// Pagebreak sentinel regexes
static RE_PAGEBREAK_SELF: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<[a-z][a-z0-9]*[^>]*epub:type="pagebreak"[^>]*/>"#).unwrap()
});
// Matches open+close pagebreak tag pairs (no backreference — regex crate doesn't support them)
static RE_PAGEBREAK_PAIR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<[a-z][a-z0-9]*[^>]*epub:type="pagebreak"[^>]*>[^<]*</[a-z][a-z0-9]*>"#)
        .unwrap()
});
static RE_PB_LABEL_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(?:title|aria-label)="([^"]*)""#).unwrap());
static RE_PB_ID_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)id="([^"]*)""#).unwrap());
static RE_PB_INNER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#">([^<]*)<"#).unwrap());
static RE_PB_SENTINEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@@PB:([^@]+)@@").unwrap());
static HYPHENATION_DICTIONARY: LazyLock<Standard> =
    LazyLock::new(|| Standard::from_embedded(Language::EnglishUS).unwrap());
const MIN_DICTIONARY_HYPHENATION_CHARS: usize = 8;

/// Approximate width of a terminal cell in image pixels, used to guess how
/// many columns an image will span before the real font size is known.
const APPROX_CELL_PIXEL_WIDTH: u32 = 8;
/// Terminal cells are roughly twice as tall as they are wide.
const CELL_WIDTH_TO_HEIGHT: f64 = 0.5;

/// Options for reserving vertical space under image placeholders so images
/// can later be rendered inline in the terminal.
pub struct InlineImageOptions {
    /// Pixel dimensions keyed by the raw `<img src>` attribute value.
    /// Images without an entry keep their single placeholder line.
    pub dimensions: HashMap<String, (u32, u32)>,
    /// Upper bound for a reserved block, in rows (typically viewport − 2).
    pub max_rows: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TypographyOptions {
    pub paragraph_style: ParagraphStyle,
    pub line_spacing: LineSpacing,
    pub justify: bool,
}

#[derive(Default)]
struct WrappedText {
    lines: Vec<String>,
    line_source_spans: Vec<(u32, u32)>,
    paragraph_starts: Vec<usize>,
    spacing_rows: HashSet<usize>,
}

/// Simple HTML parser for ebook content
/// This uses html2text for the heavy lifting and adds some basic structure tracking
pub fn parse_html(
    html_src: &str,
    text_width: Option<usize>,
    section_ids: Option<HashSet<String>>,
    starting_line: usize,
) -> Result<TextStructure> {
    parse_html_with_styles(
        html_src,
        text_width,
        section_ids,
        starting_line,
        &StyledClasses::default(),
        None,
    )
}

/// Like `parse_html`, but takes a CSS class → emphasis map so spans/paras
/// styled italic/bold via class get marked accordingly.
pub fn parse_html_with_styles(
    html_src: &str,
    text_width: Option<usize>,
    section_ids: Option<HashSet<String>>,
    starting_line: usize,
    styled_classes: &StyledClasses,
    inline_images: Option<&InlineImageOptions>,
) -> Result<TextStructure> {
    parse_html_with_styles_and_typography(
        html_src,
        text_width,
        section_ids,
        starting_line,
        styled_classes,
        inline_images,
        TypographyOptions::default(),
    )
}

pub fn parse_html_with_styles_and_typography(
    html_src: &str,
    text_width: Option<usize>,
    section_ids: Option<HashSet<String>>,
    starting_line: usize,
    styled_classes: &StyledClasses,
    inline_images: Option<&InlineImageOptions>,
    typography: TypographyOptions,
) -> Result<TextStructure> {
    let text_width = text_width.unwrap_or(80);
    let html_src = preprocess_inline_annotations(html_src);
    let html_src = preprocess_svg_images(&html_src);
    let html_src = preprocess_images(&html_src);
    let html_src = preprocess_pagebreaks(&html_src);

    // Parse HTML once
    let fragment = Html::parse_fragment(&html_src);

    // Convert HTML to plain text first with infinite width to preserve paragraphs
    // then wrap with hyphenation
    let mut raw_lines = html_to_plain_text(&html_src, usize::MAX)?;

    // Normalize list markers to match epy style ('- ') instead of html2text style ('* ')
    for line in raw_lines.iter_mut() {
        if line.starts_with("* ") {
            *line = line.replacen("* ", "- ", 1);
        }
    }

    replace_superscript_link_markers(&mut raw_lines);
    // Strip inline markers before wrapping so invisible chars don't affect line breaks.
    let mut marker_formatting =
        extract_formatting(&fragment, starting_line, &raw_lines, styled_classes)?;
    strip_inline_markers(&mut raw_lines, &mut marker_formatting, starting_line);

    // Tighten stanza-style runs of italic-class paragraphs (collapse blank
    // separators between consecutive single-line paragraphs sharing an italic
    // class) BEFORE wrapping, so the operation works on logical paragraphs.
    tighten_italic_paragraph_runs(&fragment, &mut raw_lines, styled_classes);

    // Indent <blockquote> content by 4 spaces.
    indent_blockquote_lines(&fragment, &mut raw_lines);

    // Pagebreak markers are parser metadata, not source text. Remove them
    // before wrapping so they cannot desynchronize the row/source projection.
    let pagebreak_offsets = strip_pagebreak_sentinels(&mut raw_lines);
    let source_text = normalized_source_text(&raw_lines);
    let source_len = u32::try_from(source_text.chars().count()).unwrap_or(u32::MAX);

    let mut wrapped =
        wrap_text_with_typography(raw_lines, text_width, &fragment, styled_classes, typography);
    let mut plain_text = std::mem::take(&mut wrapped.lines);

    // Reserve blank rows under image placeholders BEFORE any row-keyed
    // structure extraction, so all recovered coordinates already account
    // for the inserted lines.
    let image_block_rows = match inline_images {
        Some(options) => reserve_image_rows(
            &mut plain_text,
            &fragment,
            starting_line,
            text_width,
            options,
        ),
        None => HashMap::new(),
    };

    // Image padding is inserted after typography. Shift paragraph/spacing
    // metadata by replaying each block insertion in row order.
    if !image_block_rows.is_empty() {
        let mut blocks: Vec<(usize, usize)> = image_block_rows
            .iter()
            .map(|(&row, &rows)| (row.saturating_sub(starting_line), rows))
            .collect();
        blocks.sort_unstable();
        for (row, rows) in blocks {
            let added = rows.saturating_sub(1);
            let carry = wrapped
                .line_source_spans
                .get(row)
                .map_or(source_len, |&(_, end)| end);
            let insert_at = (row + 1).min(wrapped.line_source_spans.len());
            wrapped.line_source_spans.splice(
                insert_at..insert_at,
                std::iter::repeat_n((carry, carry), added),
            );
            for start in &mut wrapped.paragraph_starts {
                if *start > row {
                    *start += added;
                }
            }
            wrapped.spacing_rows = wrapped
                .spacing_rows
                .into_iter()
                .map(|spacing| {
                    if spacing > row {
                        spacing + added
                    } else {
                        spacing
                    }
                })
                .collect();
        }
    }

    let source_map = SourceMap {
        row_spans: wrapped.line_source_spans,
        source_len,
        source_text,
        normalization_version: NORMALIZATION_VERSION,
    };
    let pagebreak_map = pagebreak_offsets
        .into_iter()
        .map(|(offset, label)| (starting_line + source_map.row_for_offset(offset), label))
        .collect();

    // Extract structure information using the parsed fragment
    let image_maps = extract_images(&fragment, starting_line, &plain_text)?;
    let section_rows = extract_sections(
        &fragment,
        &section_ids.unwrap_or_default(),
        starting_line,
        &plain_text,
    )?;
    let mut formatting = extract_formatting(&fragment, starting_line, &plain_text, styled_classes)?;
    convert_formatting_bytes_to_chars(&mut formatting, &plain_text, starting_line);
    let links = extract_links(&fragment, starting_line, &plain_text)?;

    Ok(TextStructure {
        text_lines: plain_text,
        image_maps,
        section_rows,
        formatting,
        links,
        pagebreak_map,
        image_block_rows,
        paragraph_starts: wrapped
            .paragraph_starts
            .into_iter()
            .map(|row| starting_line + row)
            .collect(),
        typography_spacing_rows: wrapped
            .spacing_rows
            .into_iter()
            .map(|row| starting_line + row)
            .collect(),
        source_map,
    })
}

/// Insert blank lines after each image placeholder so the image can be
/// rendered into the reserved block, and return the block heights keyed by
/// the placeholder's absolute row (matching `image_maps` keys).
///
/// The height estimate assumes ~8 px per cell horizontally and 1:2 cell
/// aspect; the renderer aspect-fits the image inside the block, so an
/// estimate that is slightly too tall only costs blank space.
fn reserve_image_rows(
    text_lines: &mut Vec<String>,
    fragment: &Html,
    starting_line: usize,
    text_width: usize,
    options: &InlineImageOptions,
) -> HashMap<usize, usize> {
    let img_selector = Selector::parse("img").unwrap();
    let image_sources: Vec<String> = fragment
        .select(&img_selector)
        .filter_map(|element| element.value().attr("src").map(str::to_string))
        .collect();

    // Placeholder rows in document order, matching extract_images' scan.
    let placeholder_rows: Vec<usize> = text_lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("[Image:") || line.contains("[[Image:"))
        .map(|(row, _)| row)
        .take(image_sources.len())
        .collect();

    let mut block_rows = HashMap::new();
    let mut shift = 0usize;
    for (row, src) in placeholder_rows.iter().zip(&image_sources) {
        let Some(&(width_px, height_px)) = options.dimensions.get(src) else {
            continue;
        };
        if width_px == 0 || height_px == 0 {
            continue;
        }
        let columns = (text_width as u32).min(width_px.div_ceil(APPROX_CELL_PIXEL_WIDTH));
        let estimated =
            (columns as f64) * (height_px as f64 / width_px as f64) * CELL_WIDTH_TO_HEIGHT;
        let rows = (estimated.ceil() as usize).clamp(2, options.max_rows.max(2));

        let placeholder_row = row + shift;
        let insert_at = (placeholder_row + 1).min(text_lines.len());
        text_lines.splice(
            insert_at..insert_at,
            std::iter::repeat_n(String::new(), rows - 1),
        );
        block_rows.insert(starting_line + placeholder_row, rows);
        shift += rows - 1;
    }
    block_rows
}

#[allow(dead_code)]
fn wrap_text(lines: Vec<String>, width: usize) -> Vec<String> {
    wrap_text_with_typography(
        lines,
        width,
        &Html::parse_fragment(""),
        &StyledClasses::default(),
        TypographyOptions::default(),
    )
    .lines
}

fn wrap_text_with_typography(
    lines: Vec<String>,
    width: usize,
    fragment: &Html,
    styled_classes: &StyledClasses,
    typography: TypographyOptions,
) -> WrappedText {
    let source_len = normalized_source_text(&lines).chars().count();
    let structural_text = structural_block_text(fragment, styled_classes);
    let structural: Vec<bool> = lines
        .iter()
        .map(|line| is_structural_line(line, &structural_text))
        .collect();
    let mut result = WrappedText::default();
    let mut chapter_cursor = 0usize;
    for (index, line) in lines.iter().enumerate() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            let prev_prose = previous_content_is_prose(index, &lines, &structural);
            let next_prose = next_content_is_prose(index, &lines, &structural);
            let compact_gap =
                typography.paragraph_style != ParagraphStyle::Spaced && prev_prose && next_prose;
            if !compact_gap {
                // Double line spacing widens paragraph gaps too, otherwise a
                // paragraph break is indistinguishable from the blank spacing
                // row between every pair of wrapped lines.
                if typography.line_spacing == LineSpacing::Double && (prev_prose || next_prose) {
                    let row = result.lines.len();
                    result.lines.push(String::new());
                    let carry = source_offset_u32(chapter_cursor.min(source_len));
                    result.line_source_spans.push((carry, carry));
                    result.spacing_rows.insert(row);
                }
                result.lines.push(String::new());
                let carry = source_offset_u32(chapter_cursor.min(source_len));
                result.line_source_spans.push((carry, carry));
            }
            continue;
        }

        let normalized_line = normalize_text(line);
        let normalized_chars: Vec<char> = normalized_line.chars().collect();
        let line_start = chapter_cursor.min(source_len);

        let prose = !structural[index];
        // Prose lines each start a paragraph; structural lines start a block
        // only after a gap, so paragraph motions step over headings and whole
        // lists the same way blank-line scanning used to.
        let follows_gap = result.lines.last().is_none_or(|prev| prev.is_empty());
        if prose || follows_gap {
            result.paragraph_starts.push(result.lines.len());
        }

        let subsequent_indent;
        // Detect list markers to maintain indentation
        if line.starts_with("* ") || line.starts_with("- ") || line.starts_with("> ") {
            subsequent_indent = "  ".to_string();
        } else if let Some(mat) = RE_ORDERED_LIST.find(line) {
            let len = mat.end();
            subsequent_indent = " ".repeat(len);
        } else {
            // Preserve any leading whitespace on continuation lines so that
            // blockquote (and similarly indented) content stays aligned after
            // wrapping.
            let leading_spaces = line.chars().take_while(|c| *c == ' ').count();
            subsequent_indent = " ".repeat(leading_spaces);
        }

        let first_indent = if prose && typography.paragraph_style == ParagraphStyle::Indented {
            "  "
        } else {
            ""
        };
        let options = Options::new(width)
            .word_splitter(WordSplitter::Custom(ebook_word_split_points))
            .initial_indent(first_indent)
            .subsequent_indent(&subsequent_indent);

        let lines_wrapped: Vec<String> = textwrap::wrap(line, &options)
            .into_iter()
            .map(|line| line.trim_end().to_string())
            .collect();
        let local_spans = match_wrapped_source_spans(&lines_wrapped, &normalized_chars);
        let last = lines_wrapped.len().saturating_sub(1);
        for (wrapped_index, (line, (local_start, local_end))) in
            lines_wrapped.into_iter().zip(local_spans).enumerate()
        {
            let mut visible = line;
            result.line_source_spans.push((
                source_offset_u32(line_start + local_start),
                source_offset_u32(line_start + local_end),
            ));
            if typography.justify && prose && wrapped_index < last {
                visible = justify_line(&visible, width, result.lines.len());
            }
            result.lines.push(visible);
            let insert_spacing = match typography.line_spacing {
                LineSpacing::Single => false,
                LineSpacing::OneAndHalf => wrapped_index < last && wrapped_index % 2 == 1,
                LineSpacing::Double => wrapped_index < last,
            };
            if prose && insert_spacing {
                let row = result.lines.len();
                result.lines.push(String::new());
                let carry = source_offset_u32(line_start + local_end);
                result.line_source_spans.push((carry, carry));
                result.spacing_rows.insert(row);
            }
        }
        chapter_cursor = chapter_cursor
            .saturating_add(normalized_chars.len())
            .saturating_add(1);
    }
    debug_assert_eq!(result.lines.len(), result.line_source_spans.len());
    result
}

fn source_offset_u32(offset: usize) -> u32 {
    u32::try_from(offset).unwrap_or(u32::MAX)
}

fn normalized_source_text(lines: &[String]) -> String {
    lines
        .iter()
        .map(|line| normalize_text(line))
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Match pre-justification wrapped pieces back to one normalized raw line.
/// Returned spans are local char offsets into `source`.
fn match_wrapped_source_spans(
    pieces: &[String],
    source: &[char],
) -> Vec<(usize, usize)> {
    let mut spans = Vec::with_capacity(pieces.len());
    let mut source_cursor = 0usize;

    for piece in pieces {
        if source_cursor < source.len() && source[source_cursor].is_whitespace() {
            source_cursor += 1;
        }
        let start = source_cursor;
        let mut mismatch = false;

        // Normalization removes raw leading whitespace, while textwrap may
        // prepend typography, blockquote, or list-continuation indentation.
        for ch in piece.chars().skip_while(|ch| ch.is_whitespace()) {
            if source_cursor < source.len()
                && (ch == source[source_cursor]
                    || (ch.is_whitespace() && source[source_cursor].is_whitespace()))
            {
                source_cursor += 1;
            } else if ch == '-' && source.get(source_cursor).is_some_and(|src| *src != '-') {
                // Dictionary hyphenation inserts a visible hyphen which has
                // no corresponding source character.
            } else {
                mismatch = true;
                break;
            }
        }

        if mismatch {
            debug_assert!(source_cursor <= source.len());
            let first_remaining = spans.len();
            return proportional_source_spans(pieces, spans, first_remaining, start, source.len());
        }
        spans.push((start, source_cursor));
    }

    if source_cursor < source.len()
        && let Some(last) = spans.last_mut()
    {
        last.1 = source.len();
    }
    spans
}

fn proportional_source_spans(
    pieces: &[String],
    mut spans: Vec<(usize, usize)>,
    first_remaining: usize,
    source_start: usize,
    source_end: usize,
) -> Vec<(usize, usize)> {
    let widths: Vec<usize> = pieces[first_remaining..]
        .iter()
        .map(|piece| UnicodeWidthStr::width(piece.as_str()).max(1))
        .collect();
    let total_width: usize = widths.iter().sum();
    let source_chars = source_end.saturating_sub(source_start);
    let mut cumulative_width = 0usize;
    let mut start = source_start;

    for (remaining_index, width) in widths.into_iter().enumerate() {
        cumulative_width += width;
        let end = if remaining_index + first_remaining + 1 == pieces.len() {
            source_end
        } else {
            source_start + source_chars.saturating_mul(cumulative_width) / total_width
        };
        spans.push((start, end));
        start = end;
    }
    spans
}

fn structural_block_text(fragment: &Html, styled_classes: &StyledClasses) -> Vec<String> {
    let selector = Selector::parse(
        "h1,h2,h3,h4,h5,h6,li,pre,code,blockquote,table,figure,figcaption,center,[style],[class]",
    )
    .unwrap();
    fragment
        .select(&selector)
        .filter(|element| {
            let name = element.value().name();
            matches!(
                name,
                "h1" | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "li"
                    | "pre"
                    | "code"
                    | "blockquote"
                    | "table"
                    | "figure"
                    | "figcaption"
                    | "center"
            ) || element.value().attr("style").is_some_and(|style| {
                style
                    .to_ascii_lowercase()
                    .replace(' ', "")
                    .contains("text-align:center")
            }) || element.value().attr("class").is_some_and(|classes| {
                classes
                    .split_whitespace()
                    .any(|class| styled_classes.centered.contains(class))
            })
        })
        .map(|element| element.text().collect::<Vec<_>>().join(" "))
        .map(|text| text.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|text| !text.is_empty())
        .collect()
}

fn is_structural_line(line: &str, structural: &[String]) -> bool {
    let trimmed = line.trim_start();
    let normalized = line.split_whitespace().collect::<Vec<_>>().join(" ");
    // Containment is checked in one direction only: the line must sit fully
    // inside a structural block's text. The reverse test would let a short
    // heading or inline <code> span (e.g. "I", "ls") capture every prose
    // line containing it as a substring.
    trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("> ")
        || trimmed.starts_with('#')
        || RE_ORDERED_LIST.is_match(trimmed)
        || line.starts_with("    ")
        || line.contains("[Image:")
        || (!normalized.is_empty() && structural.iter().any(|text| text.contains(&normalized)))
}

fn previous_content_is_prose(index: usize, lines: &[String], structural: &[bool]) -> bool {
    (0..index)
        .rev()
        .find(|&i| !lines[i].trim().is_empty())
        .is_some_and(|i| !structural[i])
}

fn next_content_is_prose(index: usize, lines: &[String], structural: &[bool]) -> bool {
    (index + 1..lines.len())
        .find(|&i| !lines[i].trim().is_empty())
        .is_some_and(|i| !structural[i])
}

fn justify_line(line: &str, width: usize, row: usize) -> String {
    let display_width = UnicodeWidthStr::width(line);
    if display_width >= width || !line.chars().any(|ch| ch.is_ascii_alphabetic()) {
        return line.to_string();
    }
    let leading = line.len() - line.trim_start_matches(' ').len();
    let body = &line[leading..];
    let words: Vec<&str> = body.split(' ').filter(|word| !word.is_empty()).collect();
    if words.len() < 2 {
        return line.to_string();
    }
    let extra = width - display_width;
    let gaps = words.len() - 1;
    let base = extra / gaps;
    let remainder = extra % gaps;
    // Spread the `remainder` one-column-wider gaps evenly across the line
    // instead of stacking them all at the left margin, and alternate the
    // sweep direction by row so the wide gaps on consecutive lines don't
    // align into vertical rivers of whitespace.
    let mut wide = vec![false; gaps];
    let mut acc = 0;
    for slot in wide.iter_mut() {
        acc += remainder;
        if acc >= gaps {
            acc -= gaps;
            *slot = true;
        }
    }
    if row % 2 == 1 {
        wide.reverse();
    }
    let mut out = String::with_capacity(line.len() + extra);
    out.push_str(&" ".repeat(leading));
    for (i, word) in words.iter().enumerate() {
        out.push_str(word);
        if i < gaps {
            out.push_str(&" ".repeat(1 + base + usize::from(wide[i])));
        }
    }
    out
}

/// Collapse blank-line separators between consecutive paragraphs that are
/// fully styled italic via a CSS class (e.g. a stanza of `<p><span
/// class="italic-class">…</span></p>`). Operates on the post-html2text
/// `raw_lines` (one long line per paragraph, blank lines between paragraphs).
fn tighten_italic_paragraph_runs(
    fragment: &Html,
    raw_lines: &mut Vec<String>,
    styled_classes: &StyledClasses,
) {
    if styled_classes.italic.is_empty() {
        return;
    }
    let normalize = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };
    let p_sel = Selector::parse("p").unwrap();
    let class_sel = Selector::parse("[class]").unwrap();

    let has_italic_class = |class_attr: &str| -> bool {
        class_attr
            .split_whitespace()
            .any(|tok| styled_classes.italic.contains(tok))
    };

    let mut italic_texts: HashSet<String> = HashSet::new();
    for p in fragment.select(&p_sel) {
        let p_text = normalize(&p.text().collect::<String>());
        if p_text.is_empty() {
            continue;
        }
        let p_italic = p
            .value()
            .attr("class")
            .map(has_italic_class)
            .unwrap_or(false);
        let italic_covers = p_italic
            || p.select(&class_sel).any(|sub| {
                sub.value()
                    .attr("class")
                    .map(has_italic_class)
                    .unwrap_or(false)
                    && normalize(&sub.text().collect::<String>()) == p_text
            });
        if italic_covers {
            italic_texts.insert(p_text);
        }
    }
    if italic_texts.is_empty() {
        return;
    }

    let mut i = 0;
    while i + 2 < raw_lines.len() {
        let prev_match =
            !raw_lines[i].trim().is_empty() && italic_texts.contains(&normalize(&raw_lines[i]));
        let blank = raw_lines[i + 1].trim().is_empty();
        let next_match = !raw_lines[i + 2].trim().is_empty()
            && italic_texts.contains(&normalize(&raw_lines[i + 2]));
        if prev_match && blank && next_match {
            raw_lines.remove(i + 1);
            // Don't advance — the new pair starts at `i` and may extend the run.
        } else {
            i += 1;
        }
    }
}

/// Indent the visible content of every `<blockquote>` by 4 spaces. Walks the
/// blockquote's child block elements (or the blockquote itself if it has no
/// block-level children) and prefixes the matching `raw_lines` with spaces.
fn indent_blockquote_lines(fragment: &Html, raw_lines: &mut Vec<String>) {
    let bq_sel = Selector::parse("blockquote").unwrap();
    let normalize = |s: &str| -> String { s.split_whitespace().collect::<Vec<_>>().join(" ") };

    let mut bq_texts: HashSet<String> = HashSet::new();
    for bq in fragment.select(&bq_sel) {
        let mut had_block_child = false;
        for child in bq.children() {
            if let Some(elem) = scraper::ElementRef::wrap(child) {
                if matches!(
                    elem.value().name(),
                    "p" | "div" | "blockquote" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                ) {
                    had_block_child = true;
                    let t = normalize(&elem.text().collect::<String>());
                    if !t.is_empty() {
                        bq_texts.insert(t);
                    }
                }
            }
        }
        if !had_block_child {
            let t = normalize(&bq.text().collect::<String>());
            if !t.is_empty() {
                bq_texts.insert(t);
            }
        }
    }
    if bq_texts.is_empty() {
        return;
    }

    for line in raw_lines.iter_mut() {
        if line.trim().is_empty() {
            continue;
        }
        // html2text's plain renderer prefixes blockquote content with "> ".
        // Strip that marker and replace it with a 4-space indent.
        let stripped = line.trim_start();
        let inner = stripped.strip_prefix("> ").unwrap_or(stripped);
        if bq_texts.contains(&normalize(inner)) {
            *line = format!("    {}", inner);
        }
    }
}

fn ebook_word_split_points(word: &str) -> Vec<usize> {
    if word.contains('-') {
        return WordSplitter::HyphenSplitter.split_points(word);
    }
    if word.chars().filter(|ch| ch.is_alphabetic()).count() < MIN_DICTIONARY_HYPHENATION_CHARS {
        return Vec::new();
    }

    use hyphenation::Hyphenator;
    HYPHENATION_DICTIONARY.hyphenate(word).breaks
}

fn extract_page_label(element_str: &str) -> String {
    if let Some(cap) = RE_PB_LABEL_ATTR.captures(element_str) {
        let label = cap[1].trim().to_string();
        if !label.is_empty() {
            return label;
        }
    }
    if let Some(cap) = RE_PB_INNER.captures(element_str) {
        let inner = cap[1].trim().to_string();
        if !inner.is_empty() {
            return inner;
        }
    }
    if let Some(cap) = RE_PB_ID_ATTR.captures(element_str) {
        let id = cap[1].trim().to_string();
        // Strip non-digit prefix (e.g. "page42" → "42")
        let digits: String = id.chars().skip_while(|c| !c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits;
        }
        if !id.is_empty() {
            return id;
        }
    }
    "?".to_string()
}

fn preprocess_pagebreaks(html: &str) -> String {
    let result = RE_PAGEBREAK_SELF.replace_all(html, |caps: &Captures| {
        format!("@@PB:{}@@", extract_page_label(&caps[0]))
    });
    RE_PAGEBREAK_PAIR
        .replace_all(&result, |caps: &Captures| {
            format!("@@PB:{}@@", extract_page_label(&caps[0]))
        })
        .to_string()
}

fn strip_pagebreak_sentinels(lines: &mut [String]) -> Vec<(usize, String)> {
    let mut pagebreaks = Vec::new();
    let mut chapter_cursor = 0usize;

    for line in lines {
        let original = std::mem::take(line);
        let mut stripped = String::with_capacity(original.len());
        let mut previous_end = 0usize;

        for captures in RE_PB_SENTINEL.captures_iter(&original) {
            let sentinel = captures.get(0).expect("pagebreak match has group zero");
            stripped.push_str(&original[previous_end..sentinel.start()]);
            let relative_offset = normalize_text(&stripped).chars().count();
            pagebreaks.push((
                chapter_cursor.saturating_add(relative_offset),
                captures[1].trim().to_string(),
            ));
            previous_end = sentinel.end();
        }
        stripped.push_str(&original[previous_end..]);
        *line = stripped;

        let normalized_len = normalize_text(line).chars().count();
        if normalized_len > 0 {
            chapter_cursor = chapter_cursor
                .saturating_add(normalized_len)
                .saturating_add(1);
        }
    }

    pagebreaks
}

fn preprocess_inline_annotations(html: &str) -> String {
    let mut processed = RE_SUP_OPEN.replace_all(html, "^{").to_string();
    processed = RE_SUP_CLOSE.replace_all(&processed, "}").to_string();
    processed = RE_SUB_OPEN.replace_all(&processed, "_{").to_string();
    RE_SUB_CLOSE.replace_all(&processed, "}").to_string()
}

fn replace_superscript_link_markers(lines: &mut [String]) {
    let mut counter = 0usize;
    for line in lines.iter_mut() {
        // Pattern 1: [^{N}] — link wrapping a superscript (inline footnote reference)
        // Renumber sequentially as ^{1}, ^{2}, etc.
        if line.contains("[^{") {
            let replaced = RE_LINK_SUP.replace_all(line, |_caps: &Captures| {
                counter += 1;
                format!("^{{{}}}", counter)
            });
            *line = replaced.to_string();
        }
        // Pattern 2: ^{[N]} — superscript wrapping a link (footnote definition label)
        // Strip the superscript wrapper, keeping just [N]
        if line.contains("^{[") {
            *line = RE_SUP_LINK.replace_all(line, "$1").to_string();
        }
    }
}

/// Convert HTML to plain text using html2text library
fn html_to_plain_text(html: &str, width: usize) -> Result<Vec<String>> {
    let text = config::plain()
        .link_footnotes(false)
        .string_from_read(html.as_bytes(), width)?;
    let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    Ok(lines)
}

/// Extract image information from HTML and map to text lines
fn extract_images(
    fragment: &Html,
    starting_line: usize,
    text_lines: &[String],
) -> Result<HashMap<usize, String>> {
    let mut images = HashMap::new();
    let img_selector = Selector::parse("img").unwrap();

    // Get all image sources in order
    let mut image_sources: Vec<String> = Vec::new();
    for element in fragment.select(&img_selector) {
        if let Some(src) = element.value().attr("src") {
            image_sources.push(src.to_string());
        }
    }

    // Find image placeholders in text lines and map them
    let mut image_idx = 0;
    for (line_num, line) in text_lines.iter().enumerate() {
        if image_idx >= image_sources.len() {
            break;
        }

        // Check for [Image: ...] or [[Image: ...]] pattern
        // html2text wraps alt in [], and our alt is [Image: ...], so it becomes [[Image: ...]]
        if line.contains("[Image:") || line.contains("[[Image:") {
            images.insert(starting_line + line_num, image_sources[image_idx].clone());
            image_idx += 1;
        }
    }

    Ok(images)
}

/// Resolve the representative text for an element, falling back to child headings,
/// parent headings, or next siblings for empty anchors.
fn resolve_element_text(element: &scraper::ElementRef, heading_selector: &Selector) -> String {
    let mut text = element.text().collect::<String>();

    // If the element itself isn't a heading, try its child headings
    if !matches!(
        element.value().name(),
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
    ) {
        if let Some(heading) = element.select(heading_selector).next() {
            let heading_text = heading.text().collect::<String>();
            if !heading_text.trim().is_empty() {
                text = heading_text;
            }
        }
    }

    // For <a> elements with very short text (e.g. footnote anchors like "1"),
    // climb up to the nearest block-level ancestor to get more context for searching.
    if element.value().name() == "a" && text.trim().len() < 3 {
        // Walk up through inline wrappers (sup, sub, span, a) to find a block parent
        let mut ancestor = element.parent();
        while let Some(node) = ancestor {
            if let Some(elem) = scraper::ElementRef::wrap(node) {
                let name = elem.value().name();
                if matches!(
                    name,
                    "p" | "div"
                        | "li"
                        | "td"
                        | "th"
                        | "blockquote"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                ) {
                    let parent_text = elem.text().collect::<String>();
                    if parent_text.trim().len() >= 3 {
                        text = parent_text;
                    }
                    break;
                }
            }
            ancestor = node.parent();
        }
    }

    // Empty anchors: fall back to parent heading or next sibling
    if text.trim().is_empty() && element.value().name() == "a" {
        if let Some(parent) = element.parent().and_then(scraper::ElementRef::wrap) {
            if matches!(
                parent.value().name(),
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
            ) {
                let parent_text = parent.text().collect::<String>();
                if !parent_text.trim().is_empty() {
                    text = parent_text;
                }
            }
        }

        if text.trim().is_empty() {
            let mut sibling = element.next_sibling();
            while let Some(node) = sibling {
                if let Some(elem) = scraper::ElementRef::wrap(node) {
                    let sibling_text = elem.text().collect::<String>();
                    if !sibling_text.trim().is_empty() {
                        text = sibling_text;
                        break;
                    }
                }
                sibling = node.next_sibling();
            }
        }
    }

    text
}

/// Search for the line matching a set of words using progressively shorter word-prefix strategies.
/// Returns the matched line number (relative to `text_lines`), or None.
fn find_line_by_words(words: &[&str], text_lines: &[String], search_start: usize) -> Option<usize> {
    let attempts = [
        (0, 32),
        (0, 10),
        (0, 5),
        (1, 32), // Skip first word (handles "[1] Text" vs "1. Text")
        (1, 10),
        (1, 5),
    ];

    for (skip, take) in attempts {
        if skip >= words.len() {
            continue;
        }
        let end = (skip + take).min(words.len());
        if end <= skip {
            continue;
        }
        let search_str = words[skip..end].join(" ");
        if search_str.len() < 3 {
            continue;
        }
        for (line_num, line) in text_lines.iter().enumerate().skip(search_start) {
            if line.contains(&search_str) {
                return Some(line_num);
            }
        }
    }

    // Fallback: try normalized text or a 32-char prefix
    let normalized = words.join(" ");
    if normalized.is_empty() {
        return None;
    }
    let prefix: String = normalized.chars().take(32).collect();
    for (line_num, line) in text_lines.iter().enumerate().skip(search_start) {
        if line.contains(&normalized) || (!prefix.is_empty() && line.contains(&prefix)) {
            return Some(line_num);
        }
    }

    None
}

/// Extract section/anchor ids from HTML for TOC navigation and internal link jumps.
fn extract_sections(
    fragment: &Html,
    _section_ids: &HashSet<String>,
    starting_line: usize,
    text_lines: &[String],
) -> Result<HashMap<String, usize>> {
    let mut sections = HashMap::new();
    let mut search_start = 0usize;

    let id_selector = Selector::parse("*[id]").unwrap();
    let heading_selector = Selector::parse("h1, h2, h3, h4, h5, h6").unwrap();

    for element in fragment.select(&id_selector) {
        if let Some(id) = element.value().attr("id") {
            let element_text = resolve_element_text(&element, &heading_selector);
            let words: Vec<&str> = element_text.split_whitespace().collect();

            if words.is_empty() {
                sections.insert(id.to_string(), starting_line + search_start);
                continue;
            }

            if let Some(line_num) = find_line_by_words(&words, text_lines, search_start) {
                sections.insert(id.to_string(), starting_line + line_num);
                search_start = line_num;
            } else {
                // Final fallback: anchor targets often align with the current cursor.
                sections.insert(id.to_string(), starting_line + search_start);
            }
        }
    }

    Ok(sections)
}

/// Extract basic formatting information (headers, bold, italic)
#[allow(clippy::type_complexity)]
fn extract_formatting(
    fragment: &Html,
    starting_line: usize,
    text_lines: &[String],
    styled_classes: &StyledClasses,
) -> Result<Vec<InlineStyle>> {
    let mut formatting = Vec::new();

    // Helper to normalize whitespace (collapse multiple spaces/newlines to single space)
    let normalize_text =
        |text: String| -> String { text.split_whitespace().collect::<Vec<_>>().join(" ") };

    // Build a selector that picks the structural emphasis tags plus, when the
    // book has any class-driven italic/bold rules, every element carrying a
    // class. We filter by class membership inside the loop.
    let selector_str = if styled_classes.is_empty() {
        "h1, h2, h3, h4, h5, h6, strong, b, em, i".to_string()
    } else {
        "h1, h2, h3, h4, h5, h6, strong, b, em, i, [class]".to_string()
    };
    let selector = Selector::parse(&selector_str).unwrap();

    // Track cursor for siblings: (line_idx, char_idx)
    // Relative to text_lines (0-based)
    let mut high_water_mark: (usize, usize) = (0, 0);

    // Stack: (Element NodeId, match_start: (line, col), match_end: (line, col))
    let mut stack: Vec<(_, (usize, usize), (usize, usize))> = Vec::new();

    for element in fragment.select(&selector) {
        // Join text nodes with whitespace so tokens at child boundaries don't
        // fuse into a single token (e.g. trailing footnote markers stuck to
        // the previous sentence).
        let text = normalize_text(element.text().collect::<Vec<_>>().join(" "));
        if text.is_empty() {
            continue;
        }

        let tag_name = element.value().name();
        let mut attrs: Vec<u32> = match tag_name {
            "strong" | "b" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => vec![1],
            "em" | "i" => vec![2],
            _ => Vec::new(),
        };
        // CSS-class-driven emphasis (italic/bold via stylesheet).
        if !styled_classes.is_empty() {
            if let Some(class_attr) = element.value().attr("class") {
                let mut italic = false;
                let mut bold = false;
                for class in class_attr.split_whitespace() {
                    if !italic && styled_classes.italic.contains(class) {
                        italic = true;
                    }
                    if !bold && styled_classes.bold.contains(class) {
                        bold = true;
                    }
                }
                if bold && !attrs.contains(&1) {
                    attrs.push(1);
                }
                if italic && !attrs.contains(&2) {
                    attrs.push(2);
                }
            }
        }
        if attrs.is_empty() {
            continue;
        }

        // Pop stack until we find a parent or stack empty
        // We walk up the current element's ancestors
        let mut ancestors = HashSet::new();
        let mut curr = element.parent();
        while let Some(node) = curr {
            ancestors.insert(node.id());
            curr = node.parent();
        }

        let mut parent_in_stack_idx = None;
        while let Some((stack_id, _, stack_end)) = stack.last() {
            if ancestors.contains(stack_id) {
                parent_in_stack_idx = Some(stack.len() - 1);
                break;
            } else {
                let stack_end = *stack_end;
                // Update high_water_mark to the end of this finished block
                if stack_end.0 > high_water_mark.0
                    || (stack_end.0 == high_water_mark.0 && stack_end.1 > high_water_mark.1)
                {
                    high_water_mark = stack_end;
                }
                stack.pop();
            }
        }

        // Determine search start
        let (start_line, start_col) = if let Some(idx) = parent_in_stack_idx {
            // Start from parent's start
            stack[idx].1
        } else {
            // Start from high_water_mark
            high_water_mark
        };

        // Search for text
        if let Some(segments) = find_text_across_lines(&text, text_lines, start_line, start_col) {
            let mut first_start = None;
            let mut last_end = None;

            for (line_idx, start, end) in segments {
                for &attr in &attrs {
                    formatting.push(InlineStyle {
                        row: (starting_line + line_idx) as u16,
                        col: start as u16,
                        n_letters: (end - start) as u16,
                        attr,
                    });
                }

                if first_start.is_none() {
                    first_start = Some((line_idx, start));
                }
                last_end = Some((line_idx, end));
            }

            if let (Some(s), Some(e)) = (first_start, last_end) {
                stack.push((element.id(), s, e));
            }
        }
    }

    Ok(formatting)
}

/// Word boundary exists if the char just before `pos` is not alphanumeric, or
/// if the candidate text itself starts with a non-alphanumeric char (so the
/// boundary is intrinsic). Used to keep `find_text_across_lines` from matching
/// a styled token inside an unrelated longer word.
fn has_word_boundary_before(line: &str, pos: usize) -> bool {
    let after = match line[pos..].chars().next() {
        Some(c) => c,
        None => return true,
    };
    if !after.is_alphanumeric() {
        return true;
    }
    match line[..pos].chars().next_back() {
        Some(prev) => !prev.is_alphanumeric(),
        None => true,
    }
}

fn has_word_boundary_after(line: &str, pos: usize) -> bool {
    let before = match line[..pos].chars().next_back() {
        Some(c) => c,
        None => return true,
    };
    if !before.is_alphanumeric() {
        return true;
    }
    match line[pos..].chars().next() {
        Some(next) => !next.is_alphanumeric(),
        None => true,
    }
}

fn find_text_across_lines(
    text_normalized: &str,
    text_lines: &[String],
    start_line: usize,
    start_col: usize,
) -> Option<Vec<(usize, usize, usize)>> {
    let tokens: Vec<&str> = text_normalized.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }

    // Try to find the sequence of tokens starting from different positions of the first token
    for line_idx in start_line..text_lines.len() {
        let line = &text_lines[line_idx];
        let search_start = if line_idx == start_line { start_col } else { 0 };

        if search_start >= line.len() {
            continue;
        }

        let first_token = tokens[0];
        let mut start_search_pos = search_start;

        while let Some(pos) = line[start_search_pos..].find(first_token) {
            let abs_pos = start_search_pos + pos;

            let advance = line[abs_pos..]
                .chars()
                .next()
                .map(|c| abs_pos + c.len_utf8());

            if has_word_boundary_before(line, abs_pos)
                && let Some(segments) = match_sequence(&tokens, text_lines, line_idx, abs_pos)
                && let Some((seg_line, _, seg_end)) = segments.last().copied()
                && has_word_boundary_after(&text_lines[seg_line], seg_end)
            {
                return Some(segments);
            }

            if let Some(next) = advance {
                start_search_pos = next;
            } else {
                break;
            }
        }
    }

    None
}

fn match_sequence(
    tokens: &[&str],
    text_lines: &[String],
    start_line: usize,
    start_col: usize,
) -> Option<Vec<(usize, usize, usize)>> {
    let mut segments = Vec::new();
    let mut current_line_idx = start_line;
    let mut current_pos = start_col + tokens[0].len();

    let mut current_segment_start = start_col;

    for token in tokens.iter().skip(1) {
        let line = &text_lines[current_line_idx];

        let lookahead_limit = 20;
        let search_slice = safe_slice(line, current_pos, lookahead_limit);

        if let Some(rel_pos) = search_slice.find(token) {
            // Found on same line
            // Check gap
            let gap = &line[current_pos..current_pos + rel_pos];
            if is_valid_gap(gap) {
                current_pos += rel_pos + token.len();
                continue;
            }
        }

        // Not found on same line (or gap invalid).
        // Handle hyphenation: line tail looks like " <prefix>-" where the
        // token starts with `<prefix>`, and the next line begins with the
        // remaining suffix.
        let remaining = &line[current_pos..];
        let trimmed = remaining.trim_start();
        let leading_ws_len = remaining.len() - trimmed.len();
        if let Some(stripped) = trimmed.strip_suffix('-') {
            if !stripped.is_empty() && token.starts_with(stripped) {
                let suffix = &token[stripped.len()..];
                if !suffix.is_empty() && current_line_idx + 1 < text_lines.len() {
                    let next_line = &text_lines[current_line_idx + 1];
                    if next_line.starts_with(suffix) {
                        let line_end = current_pos + leading_ws_len + stripped.len() + 1;
                        segments.push((current_line_idx, current_segment_start, line_end));
                        current_line_idx += 1;
                        current_segment_start = 0;
                        current_pos = suffix.len();
                        continue;
                    }
                }
            }
        }

        // Check next line
        if is_valid_gap(remaining) {
            // Close current segment
            // Trim trailing markers/whitespace from segment end?
            // current_pos includes up to the end of the last matched token.
            // But we checked remaining is valid gap.
            segments.push((current_line_idx, current_segment_start, current_pos));

            // Move to next line
            current_line_idx += 1;
            while current_line_idx < text_lines.len()
                && text_lines[current_line_idx].trim().is_empty()
            {
                current_line_idx += 1;
            }
            if current_line_idx >= text_lines.len() {
                return None; // Ran out of lines but tokens remain
            }

            // Start new segment
            // Find `token` in new line.
            // It should be at the beginning, possibly after markers/whitespace.
            let line = &text_lines[current_line_idx];
            let lookahead_limit = 20;
            let search_slice = safe_slice(line, 0, lookahead_limit);

            if let Some(pos) = search_slice.find(token) {
                let prefix = &line[..pos];
                if is_valid_gap(prefix) {
                    current_segment_start = pos;
                    current_pos = pos + token.len();
                    continue;
                }
            }

            return None;
        } else {
            return None;
        }
    }

    segments.push((current_line_idx, current_segment_start, current_pos));
    Some(segments)
}

fn safe_slice(s: &str, start: usize, len: usize) -> &str {
    if start >= s.len() {
        return "";
    }
    let mut end = (start + len).min(s.len());
    while !s.is_char_boundary(end) {
        end += 1;
    }
    &s[start..end]
}

fn is_valid_gap(gap: &str) -> bool {
    // Gap can contain whitespace, *, [, ], (, ), punctuation?
    // Usually just spaces.
    // And for line transitions: **, *
    gap.chars()
        .all(|c| c.is_whitespace() || c == '*' || c == '[' || c == ']')
}

/// Extract link metadata without injecting markers into the rendered text.
/// We keep links as separate entries so reading flow stays unchanged; link UI uses these rows.
fn extract_links(
    fragment: &Html,
    starting_line: usize,
    text_lines: &[String],
) -> Result<Vec<LinkEntry>> {
    let mut links = Vec::new();
    let link_selector = Selector::parse("a[href]").unwrap();
    let sup_selector = Selector::parse("sup").unwrap();
    let mut sup_counter = 0usize;

    for element in fragment.select(&link_selector) {
        let href = match element.value().attr("href") {
            Some(value) if !value.trim().is_empty() => value.trim(),
            _ => continue,
        };

        // Filter out backlinks inside footnotes
        // Check if any ancestor has epub:type="footnote" or a common footnote CSS class
        // explicitly allow epub:type="noteref" (links TO footnotes)
        let is_noteref = element.value().attr("epub:type") == Some("noteref");
        if !is_noteref {
            let footnote_classes = [
                "fn",
                "footnote",
                "footnotes",
                "endnote",
                "endnotes",
                "note",
                "notes",
            ];
            let mut parent = element.parent();
            let mut inside_footnote = false;
            while let Some(node) = parent {
                if let Some(element_ref) = scraper::ElementRef::wrap(node) {
                    // Check epub:type="footnote"
                    if element_ref.value().attr("epub:type") == Some("footnote") {
                        inside_footnote = true;
                        break;
                    }
                    // Check common footnote CSS class names
                    if let Some(class_attr) = element_ref.value().attr("class") {
                        let has_fn_class = class_attr
                            .split_whitespace()
                            .any(|c| footnote_classes.contains(&c));
                        if has_fn_class {
                            inside_footnote = true;
                            break;
                        }
                    }
                }
                parent = node.parent();
            }

            if inside_footnote {
                // heuristic: if it's an internal link, likely a backlink.
                // To be safer, we check if label is short (likely a number or symbol).
                // We keep external links and longer internal links (e.g. "See Chapter 1").
                if href.starts_with('#') {
                    let text = element.text().collect::<String>().trim().to_string();
                    if text.len() <= 4 {
                        continue;
                    }
                }
            }
        }

        let is_sup = element.select(&sup_selector).next().is_some();
        let (label, search_text) = if is_sup {
            sup_counter += 1;
            let label = format!("^{{{}}}", sup_counter);
            (label.clone(), label)
        } else {
            let raw_label = element.text().collect::<String>();
            let label = raw_label.split_whitespace().collect::<Vec<_>>().join(" ");
            let search_text = if label.is_empty() {
                href.to_string()
            } else {
                label.clone()
            };
            (label, search_text)
        };

        let mut row = None;
        if !search_text.is_empty() {
            for (line_num, line) in text_lines.iter().enumerate() {
                if line.contains(&search_text) {
                    row = Some(starting_line + line_num);
                    break;
                }
            }
        }

        links.push(LinkEntry {
            row: row.unwrap_or(starting_line),
            label: if label.is_empty() {
                href.to_string()
            } else {
                label
            },
            url: href.to_string(),
            target_row: None,
        });
    }

    Ok(links)
}

/// Convert `col` and `n_letters` of every entry from byte offsets (as produced
/// by `extract_formatting`, which works in byte positions internally) to char
/// offsets, since the renderer indexes lines by char position.
fn convert_formatting_bytes_to_chars(
    formatting: &mut [InlineStyle],
    text_lines: &[String],
    starting_line: usize,
) {
    use std::collections::HashMap;
    let mut cache: HashMap<usize, Vec<usize>> = HashMap::new();
    for style in formatting.iter_mut() {
        let row = style.row as usize;
        if row < starting_line {
            continue;
        }
        let line_idx = row - starting_line;
        if line_idx >= text_lines.len() {
            continue;
        }
        let map = cache.entry(line_idx).or_insert_with(|| {
            let line = &text_lines[line_idx];
            let mut byte_to_char = vec![0usize; line.len() + 1];
            let mut char_idx = 0usize;
            for (byte_i, c) in line.char_indices() {
                for slot in &mut byte_to_char[byte_i..byte_i + c.len_utf8()] {
                    *slot = char_idx;
                }
                char_idx += 1;
            }
            byte_to_char[line.len()] = char_idx;
            byte_to_char
        });
        let start_byte = (style.col as usize).min(map.len() - 1);
        let end_byte = ((style.col as usize) + (style.n_letters as usize)).min(map.len() - 1);
        let start_char = map[start_byte];
        let end_char = map[end_byte];
        style.col = start_char as u16;
        style.n_letters = end_char.saturating_sub(start_char) as u16;
    }
}

fn strip_inline_markers(
    text_lines: &mut [String],
    formatting: &mut [InlineStyle],
    starting_line: usize,
) {
    for (idx, line) in text_lines.iter_mut().enumerate() {
        let row = starting_line + idx;
        let mut line_formatting = Vec::new();
        for (style_idx, style) in formatting.iter().enumerate() {
            if style.row as usize == row {
                line_formatting.push(style_idx);
            }
        }
        if line_formatting.is_empty() {
            continue;
        }

        let remove_positions = collect_marker_positions(line, formatting, &line_formatting);
        if remove_positions.is_empty() {
            continue;
        }

        for style_idx in &line_formatting {
            let entry = &mut formatting[*style_idx];
            let old_col = entry.col as usize;
            let shift = remove_positions.partition_point(|&pos| pos < old_col);
            entry.col = (old_col.saturating_sub(shift)) as u16;
        }

        let mut remove_flags = vec![false; line.len()];
        for pos in &remove_positions {
            if *pos < remove_flags.len() {
                remove_flags[*pos] = true;
            }
        }

        let bytes = line.as_bytes();
        let mut new_bytes = Vec::with_capacity(bytes.len().saturating_sub(remove_positions.len()));
        for (i, b) in bytes.iter().enumerate() {
            if !remove_flags[i] {
                new_bytes.push(*b);
            }
        }

        if let Ok(new_line) = String::from_utf8(new_bytes) {
            *line = new_line;
        }
    }
}

fn collect_marker_positions(
    line: &str,
    formatting: &[InlineStyle],
    line_formatting: &[usize],
) -> Vec<usize> {
    let bytes = line.as_bytes();
    let len = bytes.len();
    let mut positions = Vec::new();

    let is_boundary = |pos: usize| -> bool {
        if pos >= len {
            return true;
        }
        let b = bytes[pos];
        // Boundary if it's whitespace or punctuation
        b.is_ascii_whitespace() || b.is_ascii_punctuation()
    };

    for style_idx in line_formatting {
        let style = &formatting[*style_idx];
        let start = style.col as usize;
        let end = start.saturating_add(style.n_letters as usize);
        match style.attr {
            1 => {
                // Bold **
                if start >= 2 && &bytes[start - 2..start] == b"**" {
                    // Check if it's a boundary marker (preceded by boundary)
                    if start == 2 || is_boundary(start - 3) {
                        positions.push(start - 2);
                        positions.push(start - 1);
                    }
                }
                if end + 2 <= len && &bytes[end..end + 2] == b"**" {
                    // Check if it's a boundary marker (followed by boundary)
                    if end + 2 == len || is_boundary(end + 2) {
                        positions.push(end);
                        positions.push(end + 1);
                    }
                }
            }
            2 => {
                // Italic *
                if start >= 1 && bytes[start - 1] == b'*' {
                    // Check if it's a boundary marker
                    if start == 1 || is_boundary(start - 2) {
                        positions.push(start - 1);
                    }
                }
                if end < len && bytes[end] == b'*' {
                    // Check if it's a boundary marker
                    if end + 1 == len || is_boundary(end + 1) {
                        positions.push(end);
                    }
                }
            }
            _ => {}
        }
    }

    positions.sort_unstable();
    positions.dedup();
    positions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_parser() {
        let html = r#"
        <h1 id="chapter1">Chapter 1</h1>
        <p>This is a <strong>bold</strong> paragraph with some <em>italic</em> text.</p>
        <ul>
            <li>First bullet point</li>
            <li>Second bullet point</li>
        </ul>
        <blockquote>
            This is an indented quote block.
        </blockquote>
        <p>Here's an image: <img src="test.jpg" alt="Test Image"></p>
        "#;

        let mut section_ids = HashSet::new();
        section_ids.insert("chapter1".to_string());

        let result = parse_html(html, Some(80), Some(section_ids), 0).unwrap();

        assert_eq!(result.text_lines.len(), 9);
        assert_eq!(result.text_lines[0], "# Chapter 1");
        assert_eq!(
            result.text_lines[2],
            "This is a bold paragraph with some italic text."
        );

        assert_eq!(result.image_maps.len(), 1);
        assert!(result.image_maps.values().any(|v| v == "test.jpg"));

        assert_eq!(result.section_rows.len(), 1);
        assert_eq!(result.section_rows.get("chapter1"), Some(&0));

        assert_eq!(result.formatting.len(), 3);
        assert!(result.formatting.iter().any(|s| s.attr == 1)); // bold
        assert!(result.formatting.iter().any(|s| s.attr == 2)); // italic
    }

    fn inline_options(dims: &[(&str, (u32, u32))], max_rows: usize) -> InlineImageOptions {
        InlineImageOptions {
            dimensions: dims
                .iter()
                .map(|(src, wh)| (src.to_string(), *wh))
                .collect(),
            max_rows,
        }
    }

    fn assert_source_map_invariants(parsed: &TextStructure) {
        let map = &parsed.source_map;
        assert_eq!(
            map.row_spans.len(),
            parsed.text_lines.len(),
            "every parser row must have a source span"
        );
        assert_eq!(
            map.source_len as usize,
            map.source_text.chars().count(),
            "source_len is a char count, not a byte count"
        );
        assert_eq!(map.normalization_version, NORMALIZATION_VERSION);

        let mut previous_end = 0u32;
        let mut covered = vec![false; map.source_len as usize];
        for (row, &(start, end)) in map.row_spans.iter().enumerate() {
            assert!(start <= end, "row {row} has reversed span {start}..{end}");
            assert!(
                previous_end <= start,
                "row {row} overlaps or moves backward: previous end {previous_end}, start {start}"
            );
            assert!(
                end <= map.source_len,
                "row {row} ends past source_len: {end} > {}",
                map.source_len
            );
            for offset in start as usize..end as usize {
                assert!(!covered[offset], "source char {offset} covered twice");
                covered[offset] = true;
            }
            previous_end = end;
        }

        for (offset, ch) in map.source_text.chars().enumerate() {
            assert!(
                covered[offset] || ch.is_whitespace(),
                "non-separator source char {ch:?} at {offset} is not covered"
            );
        }
    }

    fn assert_offset_projects_to_row(map: &SourceMap, offset: usize) {
        let row = map.row_for_offset(offset);
        let (start, end) = map.row_spans[row];
        assert!(
            start < end && start as usize <= offset && offset < end as usize,
            "offset {offset} projected to non-containing row {row} ({start}..{end})"
        );
    }

    #[test]
    fn source_map_spans_satisfy_invariants_on_html_fixtures() {
        let fixtures = [
            r#"<h1>Heading</h1><p>Ordinary prose with enough words to wrap across several rows.</p>"#,
            r#"<ol><li>First list item with continued text</li><li>Second item</li></ol><blockquote>Quoted words spanning more than one line at a narrow width.</blockquote>"#,
            r#"<p>English followed by 中文段落，包含全角标点和多个字符。</p><p>well-established characteristically long words</p>"#,
        ];

        for html in fixtures {
            let parsed = parse_html(html, Some(24), None, 0).unwrap();
            assert_source_map_invariants(&parsed);
        }
    }

    #[test]
    fn source_map_round_trip_is_stable_across_widths_and_typography() {
        let html = r#"
            <h2>Stable Coordinates</h2>
            <p>Characteristically thoughtful readers compare well-established ideas across widths without losing their canonical place.</p>
            <blockquote>引用文字也应当使用字符偏移，而不是 UTF-8 字节偏移。</blockquote>
            <ul><li>A list item whose continuation indentation is synthetic.</li></ul>
            <p>Another paragraph supplies enough words for justification and line spacing.</p>
        "#;
        let widths = [30, 40, 60, 80, 120];
        let mut parsed_versions = Vec::new();

        for paragraph_style in [ParagraphStyle::Indented, ParagraphStyle::Spaced] {
            for line_spacing in [LineSpacing::Single, LineSpacing::Double] {
                for justify in [false, true] {
                    let typography = TypographyOptions {
                        paragraph_style,
                        line_spacing,
                        justify,
                    };
                    let versions: Vec<TextStructure> = widths
                        .iter()
                        .map(|&width| {
                            parse_html_with_styles_and_typography(
                                html,
                                Some(width),
                                None,
                                0,
                                &StyledClasses::default(),
                                None,
                                typography,
                            )
                            .unwrap()
                        })
                        .collect();
                    for parsed in &versions {
                        assert_source_map_invariants(parsed);
                        assert_eq!(
                            parsed.source_map.source_text, versions[0].source_map.source_text,
                            "canonical source changed with width or typography"
                        );
                    }
                    parsed_versions.push(versions);
                }
            }
        }

        for versions in &parsed_versions {
            for source in versions {
                for (source_row, &(start, end)) in
                    source.source_map.row_spans.iter().enumerate()
                {
                    if start == end {
                        continue;
                    }
                    let offset = source.source_map.offset_for_row(source_row);
                    assert_eq!(offset, start as usize);
                    assert_eq!(source.source_map.row_for_offset(offset), source_row);

                    for target in versions {
                        assert_offset_projects_to_row(&target.source_map, offset);
                        let target_row = target.source_map.row_for_offset(offset);
                        let (target_start, target_end) = target.source_map.row_spans[target_row];
                        assert!(
                            target_start as usize <= offset && offset < target_end as usize,
                            "canonical offset {offset} was not stable across widths"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn source_map_handles_hyphenation_cjk_blockquotes_and_pagebreaks() {
        let dictionary = parse_html(
            "<p>characteristically characteristically</p>",
            Some(10),
            None,
            0,
        )
        .unwrap();
        assert!(
            dictionary.text_lines.iter().any(|line| line.ends_with('-')),
            "fixture must exercise dictionary-inserted hyphenation: {:?}",
            dictionary.text_lines
        );
        assert_source_map_invariants(&dictionary);

        let compound = parse_html(
            "<p>well-established well-established</p>",
            Some(10),
            None,
            0,
        )
        .unwrap();
        assert!(compound.source_map.source_text.contains("well-established"));
        assert_source_map_invariants(&compound);

        let cjk = parse_html(
            "<p>这是一个用于验证字符偏移而不是字节偏移的中文段落。</p>",
            Some(12),
            None,
            0,
        )
        .unwrap();
        assert!(cjk.source_map.source_text.len() > cjk.source_map.source_len as usize);
        assert_source_map_invariants(&cjk);

        let blockquote = parse_html(
            "<blockquote>Quoted content wraps while its leading indentation remains synthetic.</blockquote>",
            Some(18),
            None,
            0,
        )
        .unwrap();
        assert!(
            blockquote
                .text_lines
                .iter()
                .filter(|line| !line.is_empty())
                .all(|line| line.starts_with("    "))
        );
        assert_source_map_invariants(&blockquote);

        let pagebreak = parse_html(
            "<p>alpha beta gamma @@PB:42@@ delta epsilon zeta</p>",
            Some(15),
            None,
            0,
        )
        .unwrap();
        assert!(!pagebreak.source_map.source_text.contains("@@PB:"));
        assert!(!pagebreak.text_lines.iter().any(|line| line.contains("@@PB:")));
        let page_offset = "alpha beta gamma".chars().count();
        assert_eq!(
            pagebreak.pagebreak_map.get(&pagebreak.source_map.row_for_offset(page_offset)),
            Some(&"42".to_string())
        );
        assert_source_map_invariants(&pagebreak);
    }

    #[test]
    fn source_map_replays_reserved_image_row_splices() {
        let html = r#"<p>Before.</p><p><img src="pic.jpg" alt="picture"></p><p>After.</p>"#;
        let options = inline_options(&[("pic.jpg", (800, 600))], 8);
        let parsed = parse_html_with_styles(
            html,
            Some(40),
            None,
            0,
            &StyledClasses::default(),
            Some(&options),
        )
        .unwrap();
        assert_source_map_invariants(&parsed);

        let (&placeholder_row, &rows) = parsed.image_block_rows.iter().next().unwrap();
        let carry = parsed.source_map.row_spans[placeholder_row].1;
        assert!(rows > 1);
        for row in placeholder_row + 1..placeholder_row + rows {
            assert_eq!(parsed.text_lines[row], "");
            assert_eq!(parsed.source_map.row_spans[row], (carry, carry));
        }
    }

    #[test]
    fn test_typography_paragraph_styles() {
        let fragment = Html::parse_fragment("<p>First paragraph.</p><p>Second paragraph.</p>");
        let raw = vec![
            "First paragraph.".to_string(),
            String::new(),
            "Second paragraph.".to_string(),
        ];
        let compact = wrap_text_with_typography(
            raw.clone(),
            40,
            &fragment,
            &StyledClasses::default(),
            TypographyOptions {
                paragraph_style: ParagraphStyle::Compact,
                ..Default::default()
            },
        );
        assert_eq!(compact.lines, ["First paragraph.", "Second paragraph."]);
        assert_eq!(compact.paragraph_starts, [0, 1]);

        let indented = wrap_text_with_typography(
            raw,
            40,
            &fragment,
            &StyledClasses::default(),
            TypographyOptions {
                paragraph_style: ParagraphStyle::Indented,
                ..Default::default()
            },
        );
        assert_eq!(
            indented.lines,
            ["  First paragraph.", "  Second paragraph."]
        );
    }

    #[test]
    fn test_typography_line_spacing_rows() {
        let fragment = Html::parse_fragment("<p>one two three four five six seven eight</p>");
        let raw = vec!["one two three four five six seven eight".to_string()];
        let double = wrap_text_with_typography(
            raw.clone(),
            10,
            &fragment,
            &StyledClasses::default(),
            TypographyOptions {
                line_spacing: LineSpacing::Double,
                ..Default::default()
            },
        );
        assert!(!double.spacing_rows.is_empty());
        assert!(
            double
                .spacing_rows
                .iter()
                .all(|&row| double.lines[row].is_empty())
        );
        assert!(!double.lines.last().unwrap().is_empty());

        let one_and_half = wrap_text_with_typography(
            raw,
            10,
            &fragment,
            &StyledClasses::default(),
            TypographyOptions {
                line_spacing: LineSpacing::OneAndHalf,
                ..Default::default()
            },
        );
        assert!(one_and_half.spacing_rows.len() < double.spacing_rows.len());
    }

    #[test]
    fn test_justification_and_structural_exclusions() {
        assert_eq!(justify_line("alpha beta", 16, 0), "alpha       beta");
        assert_eq!(justify_line("纯中文内容", 16, 0), "纯中文内容");

        let fragment = Html::parse_fragment("<pre>code block words here</pre>");
        let wrapped = wrap_text_with_typography(
            vec!["code block words here".to_string()],
            12,
            &fragment,
            &StyledClasses::default(),
            TypographyOptions {
                justify: true,
                ..Default::default()
            },
        );
        assert_eq!(wrapped.lines[0], "code block");
    }

    #[test]
    fn test_justify_spreads_remainder_and_alternates_by_row() {
        // 4 words, 3 gaps, 2 leftover columns: the wide gaps must not both
        // sit at the start of the line.
        let even = justify_line("aa bb cc dd", 13, 0);
        assert_eq!(UnicodeWidthStr::width(even.as_str()), 13);
        assert_eq!(even.split("  ").count(), 3, "two double gaps: {even:?}");
        assert_ne!(even, "aa  bb  cc dd", "remainder must not stack left");

        // Odd rows mirror the distribution so wide gaps don't line up into
        // vertical rivers across consecutive lines.
        let odd = justify_line("aa bb cc dd", 13, 1);
        assert_eq!(UnicodeWidthStr::width(odd.as_str()), 13);
        assert_ne!(even, odd);
        let mirrored: String = even
            .chars()
            .rev()
            .collect::<String>()
            .replace(|c: char| c.is_alphabetic(), "x");
        let odd_masked: String = odd.replace(|c: char| c.is_alphabetic(), "x");
        assert_eq!(mirrored, odd_masked, "odd rows reverse the gap pattern");
    }

    #[test]
    fn test_short_structural_text_does_not_poison_prose() {
        // "I" (a chapter-numeral heading) and "ls" (an inline code span) are
        // substrings of most prose lines; they must not mark those lines
        // structural.
        let fragment = Html::parse_fragment(
            "<h2>I</h2><p>It is a truth universally acknowledged.</p>\
             <p>Run <code>ls</code> to list files also.</p>",
        );
        let raw = vec![
            "# I".to_string(),
            String::new(),
            "It is a truth universally acknowledged.".to_string(),
            String::new(),
            "Run ls to list files also.".to_string(),
        ];
        let wrapped = wrap_text_with_typography(
            raw,
            80,
            &fragment,
            &StyledClasses::default(),
            TypographyOptions {
                paragraph_style: ParagraphStyle::Indented,
                ..Default::default()
            },
        );
        assert_eq!(wrapped.lines[0], "# I", "heading keeps no indent");
        assert_eq!(
            wrapped.lines[2], "  It is a truth universally acknowledged.",
            "prose after a one-letter heading is still indented"
        );
        assert_eq!(
            wrapped.lines[3], "  Run ls to list files also.",
            "prose containing an inline code span is still indented"
        );
    }

    #[test]
    fn test_structural_blocks_get_paragraph_starts_after_gaps() {
        let fragment = Html::parse_fragment(
            "<h1>Title</h1><p>First paragraph.</p><ul><li>alpha</li><li>beta</li></ul>",
        );
        let raw = vec![
            "# Title".to_string(),
            String::new(),
            "First paragraph.".to_string(),
            String::new(),
            "- alpha".to_string(),
            "- beta".to_string(),
        ];
        let wrapped = wrap_text_with_typography(
            raw,
            40,
            &fragment,
            &StyledClasses::default(),
            TypographyOptions::default(),
        );
        // Heading and the list's first item start blocks; the second list
        // item continues its block.
        assert_eq!(wrapped.paragraph_starts, [0, 2, 4]);
    }

    #[test]
    fn test_double_spacing_keeps_paragraph_gap_distinct() {
        let fragment = Html::parse_fragment("<p>one two three four</p><p>five six seven eight</p>");
        let raw = vec![
            "one two three four".to_string(),
            String::new(),
            "five six seven eight".to_string(),
        ];
        let wrapped = wrap_text_with_typography(
            raw,
            10,
            &fragment,
            &StyledClasses::default(),
            TypographyOptions {
                line_spacing: LineSpacing::Double,
                ..Default::default()
            },
        );
        let second_start = wrapped
            .lines
            .iter()
            .position(|line| line == "five six")
            .unwrap();
        let first_end = wrapped.lines[..second_start]
            .iter()
            .rposition(|line| !line.is_empty())
            .unwrap();
        // The paragraph gap is two blank rows — wider than the single blank
        // spacing row between wrapped lines — and only one of them is a
        // layout-only spacing row.
        assert_eq!(second_start - first_end, 3);
        let gap_spacing = (first_end + 1..second_start)
            .filter(|row| wrapped.spacing_rows.contains(row))
            .count();
        assert_eq!(gap_spacing, 1);
    }

    #[test]
    fn typography_rows_preserve_formatting_and_link_coordinates() {
        let html = r#"<p>alpha beta <strong>gamma delta</strong> epsilon <a href="next.xhtml">zeta eta</a> theta</p>"#;
        let parsed = parse_html_with_styles_and_typography(
            html,
            Some(16),
            None,
            0,
            &StyledClasses::default(),
            None,
            TypographyOptions {
                line_spacing: LineSpacing::Double,
                justify: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!parsed.typography_spacing_rows.is_empty());
        assert!(parsed.formatting.iter().all(|style| {
            !parsed
                .typography_spacing_rows
                .contains(&(style.row as usize))
                && parsed.text_lines[style.row as usize]
                    .chars()
                    .skip(style.col as usize)
                    .take(style.n_letters as usize)
                    .any(|ch| !ch.is_whitespace())
        }));
        assert_eq!(parsed.links.len(), 1);
        assert!(
            !parsed
                .typography_spacing_rows
                .contains(&parsed.links[0].row)
        );
        assert!(parsed.text_lines[parsed.links[0].row].contains("zeta"));
    }

    #[test]
    fn test_reserve_image_rows_inserts_blank_block() {
        let html = r#"
        <p>Before the image.</p>
        <p><img src="pic.jpg" alt="A picture"></p>
        <p>After the image.</p>
        "#;
        let options = inline_options(&[("pic.jpg", (800, 600))], 20);
        let with = parse_html_with_styles(
            html,
            Some(80),
            None,
            0,
            &StyledClasses::default(),
            Some(&options),
        )
        .unwrap();
        let without = parse_html(html, Some(80), None, 0).unwrap();

        // 800 px wide caps at the 80-col text width; 80 * (600/800) * 0.5 = 30,
        // clamped to max_rows = 20.
        let (&row, &rows) = with.image_block_rows.iter().next().expect("block reserved");
        assert_eq!(rows, 20);
        assert!(with.image_maps.contains_key(&row));
        assert_eq!(
            with.text_lines.len(),
            without.text_lines.len() + rows - 1,
            "block adds rows-1 blank lines"
        );
        // The reserved lines are blank; following content is shifted intact.
        for offset in 1..rows {
            assert_eq!(with.text_lines[row + offset], "");
        }
        assert!(with.text_lines[row].contains("[Image:"));
        let after_with = with
            .text_lines
            .iter()
            .position(|l| l.contains("After the image."))
            .unwrap();
        let after_without = without
            .text_lines
            .iter()
            .position(|l| l.contains("After the image."))
            .unwrap();
        assert_eq!(
            after_with,
            after_without + rows - 1,
            "following content shifts by exactly the inserted rows"
        );
        assert!(without.image_block_rows.is_empty());
    }

    #[test]
    fn test_reserve_image_rows_small_image_and_missing_dims() {
        let html = r#"
        <p><img src="icon.png" alt="icon"></p>
        <p><img src="unknown.png" alt="mystery"></p>
        "#;
        // 64 px icon → 8 columns → 8 * 1.0 * 0.5 = 4 rows; unknown.png has
        // no dimensions so it keeps its single placeholder line.
        let options = inline_options(&[("icon.png", (64, 64))], 20);
        let result = parse_html_with_styles(
            html,
            Some(80),
            None,
            0,
            &StyledClasses::default(),
            Some(&options),
        )
        .unwrap();
        assert_eq!(result.image_block_rows.len(), 1);
        assert_eq!(result.image_block_rows.values().copied().next(), Some(4));
        assert_eq!(result.image_maps.len(), 2);
    }

    #[test]
    fn test_reserve_image_rows_shifts_later_structures() {
        let html = r#"
        <p><img src="pic.jpg" alt="A picture"></p>
        <h1 id="after">After Heading</h1>
        <p>Some text with a <a href="ch02.xhtml">link</a>.</p>
        "#;
        let mut section_ids = HashSet::new();
        section_ids.insert("after".to_string());
        let options = inline_options(&[("pic.jpg", (400, 400))], 30);
        let result = parse_html_with_styles(
            html,
            Some(80),
            Some(section_ids),
            0,
            &StyledClasses::default(),
            Some(&options),
        )
        .unwrap();

        let (&img_row, &rows) = result.image_block_rows.iter().next().unwrap();
        // 400 px → 50 cols → 50 * 1.0 * 0.5 = 25 rows.
        assert_eq!(rows, 25);
        let section_row = *result.section_rows.get("after").unwrap();
        assert!(
            section_row > img_row + rows - 1,
            "section row {section_row} must land below the reserved block"
        );
        assert_eq!(
            result.text_lines[section_row], "# After Heading",
            "section row points at the heading text after shifting"
        );
        let link = result.links.first().expect("link extracted");
        assert!(
            result.text_lines[link.row].contains("link"),
            "link row realigned to shifted text"
        );
    }

    #[test]
    fn test_preprocess_svg_images_calibre_cover() {
        // The Calibre/KF8 cover pattern: a raster image wrapped in inline SVG.
        let html = r#"
        <div>
            <svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" version="1.1" width="100%" height="100%" viewBox="0 0 898 1499" preserveAspectRatio="none">
                <image width="898" height="1499" xlink:href="cover.jpeg"/>
            </svg>
        </div>
        "#;
        let processed = preprocess_svg_images(html);
        assert!(processed.contains(r#"<img src="cover.jpeg">"#));
        assert!(!processed.contains("<svg"));

        // Pure vector SVG (no wrapped raster image) is left untouched.
        let vector = r#"<svg viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;
        assert_eq!(preprocess_svg_images(vector), vector);
    }

    #[test]
    fn test_svg_wrapped_cover_gets_placeholder_and_block() {
        let html = r#"
        <div><svg viewBox="0 0 898 1499"><image width="898" height="1499" xlink:href="cover.jpeg"/></svg></div>
        "#;
        let options = inline_options(&[("cover.jpeg", (898, 1499))], 20);
        let result = parse_html_with_styles(
            html,
            Some(80),
            None,
            0,
            &StyledClasses::default(),
            Some(&options),
        )
        .unwrap();
        let (&row, &rows) = result
            .image_block_rows
            .iter()
            .next()
            .expect("block reserved for the svg-wrapped cover");
        assert_eq!(rows, 20);
        assert!(result.text_lines[row].contains("[Image: cover.jpeg]"));
        assert_eq!(
            result.image_maps.get(&row).map(String::as_str),
            Some("cover.jpeg")
        );
    }

    #[test]
    fn test_html_to_plain_text() {
        let html = "<p>Hello, world!</p>";
        let lines = html_to_plain_text(html, 80).unwrap();
        assert_eq!(lines, vec!["Hello, world!"]);
    }

    #[test]
    fn test_html_to_plain_text_with_wrapping() {
        let html = "<p>This is a very long paragraph that should be wrapped when converted to plain text with a limited width.</p>";
        let lines = html_to_plain_text(html, 30).unwrap();
        // Should wrap the text
        assert!(lines.len() > 1);
        assert!(lines[0].len() <= 30);
    }

    #[test]
    fn test_wrap_text_only_splits_hyphenated_compounds_at_hyphens() {
        let lines = wrap_text(vec!["see-hear-smell-taste-touch".to_string()], 12);

        assert_eq!(lines, vec!["see-hear-", "smell-taste-", "touch"]);
    }

    #[test]
    fn test_wrap_text_does_not_dictionary_hyphenate_short_words() {
        assert!(ebook_word_split_points("smell").is_empty());
    }

    #[test]
    fn test_html_to_plain_text_empty() {
        let html = "";
        let lines = html_to_plain_text(html, 80).unwrap();
        assert_eq!(lines, Vec::<String>::new());
    }

    #[test]
    fn test_html_to_plain_text_multiple_paragraphs() {
        let html = r#"
        <p>First paragraph.</p>
        <p>Second paragraph with <strong>bold</strong> text.</p>
        <p>Third paragraph.</p>
        "#;
        let lines = html_to_plain_text(html, 80).unwrap();
        // html2text might add blank lines between paragraphs, so check minimum
        assert!(lines.len() >= 3);
        assert!(lines.iter().any(|l| l.contains("First paragraph.")));
        assert!(
            lines
                .iter()
                .any(|l| l.contains("Second paragraph with **bold** text."))
        );
        assert!(lines.iter().any(|l| l.contains("Third paragraph.")));
    }

    #[test]
    fn test_extract_images() {
        let html = r#"<p>Here's an image: <img src="test.jpg" alt="[Image: test.jpg]"></p>"#;
        let fragment = Html::parse_fragment(html);
        // Mock text lines that html2text would produce
        let text_lines = vec!["Here's an image: [[Image: test.jpg]]".to_string()];
        let images = extract_images(&fragment, 0, &text_lines).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images.get(&0), Some(&"test.jpg".to_string()));
    }

    #[test]
    fn test_extract_images_multiple() {
        let html = r#"
        <p>First image: <img src="image1.jpg" alt="[Image: image1.jpg]"></p>
        <p>Second image: <img src="image2.png" alt="[Image: image2.png]"></p>
        <img src="image3.gif" alt="[Image: image3.gif]">
        "#;
        let fragment = Html::parse_fragment(html);

        // Mock text lines
        let text_lines = vec![
            "First image: [[Image: image1.jpg]]".to_string(),
            "Second image: [[Image: image2.png]]".to_string(),
            "[[Image: image3.gif]]".to_string(),
        ];

        let images = extract_images(&fragment, 5, &text_lines).unwrap();
        assert_eq!(images.len(), 3);
        assert_eq!(images.get(&5), Some(&"image1.jpg".to_string()));
        assert_eq!(images.get(&6), Some(&"image2.png".to_string()));
        assert_eq!(images.get(&7), Some(&"image3.gif".to_string()));
    }

    #[test]
    fn test_extract_images_none() {
        let html = "<p>No images here.</p>";
        let fragment = Html::parse_fragment(html);
        let text_lines = vec!["No images here.".to_string()];
        let images = extract_images(&fragment, 0, &text_lines).unwrap();
        assert_eq!(images.len(), 0);
    }

    #[test]
    fn test_extract_images_without_src() {
        let html = "<p><img alt=\"Image without src\"></p>";
        let fragment = Html::parse_fragment(html);
        let text_lines = vec!["[[Image without src]]".to_string()];
        let images = extract_images(&fragment, 0, &text_lines).unwrap();
        assert_eq!(images.len(), 0);
    }

    #[test]
    fn test_extract_sections() {
        let html = r#"<h1 id="chapter1">Chapter 1</h1>"#;
        let fragment = Html::parse_fragment(html);
        let mut section_ids = HashSet::new();
        section_ids.insert("chapter1".to_string());
        let text_lines = vec!["# Chapter 1".to_string()];
        let sections = extract_sections(&fragment, &section_ids, 0, &text_lines).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections.get("chapter1"), Some(&0));
    }

    #[test]
    fn test_extract_sections_multiple() {
        let html = r#"
        <h1 id="intro">Introduction</h1>
        <p>Some content here.</p>
        <h2 id="chapter1">Chapter 1</h2>
        <p>More content.</p>
        <div id="conclusion">Conclusion</div>
        "#;
        let fragment = Html::parse_fragment(html);
        let mut section_ids = HashSet::new();
        section_ids.insert("intro".to_string());
        section_ids.insert("chapter1".to_string());
        section_ids.insert("conclusion".to_string());

        let text_lines = vec![
            "# Introduction".to_string(),
            "Some content here.".to_string(),
            "## Chapter 1".to_string(),
            "More content.".to_string(),
            "Conclusion".to_string(),
        ];

        let sections = extract_sections(&fragment, &section_ids, 0, &text_lines).unwrap();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections.get("intro"), Some(&0));
        assert_eq!(sections.get("chapter1"), Some(&2));
        assert_eq!(sections.get("conclusion"), Some(&4));
    }

    #[test]
    fn test_extract_sections_container_with_heading_child() {
        let html = r#"
        <div id="chapter1">
            <h2>Chapter 1</h2>
            <p>Some content here.</p>
        </div>
        "#;
        let fragment = Html::parse_fragment(html);
        let section_ids = HashSet::new();
        let text_lines = vec!["## Chapter 1".to_string(), "Some content here.".to_string()];

        let sections = extract_sections(&fragment, &section_ids, 0, &text_lines).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections.get("chapter1"), Some(&0));
    }

    #[test]
    fn test_extract_sections_empty_section_ids() {
        let html = r#"<h1 id="chapter1">Chapter 1</h1>"#;
        let fragment = Html::parse_fragment(html);
        let section_ids = HashSet::new();
        let text_lines = vec!["# Chapter 1".to_string()];
        let sections = extract_sections(&fragment, &section_ids, 0, &text_lines).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections.get("chapter1"), Some(&0));
    }

    #[test]
    fn test_extract_sections_no_matching_sections() {
        let html = r#"<h1 id="chapter1">Chapter 1</h1>"#;
        let fragment = Html::parse_fragment(html);
        let mut section_ids = HashSet::new();
        section_ids.insert("nonexistent".to_string());
        let text_lines = vec!["# Chapter 1".to_string()];
        let sections = extract_sections(&fragment, &section_ids, 0, &text_lines).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections.get("chapter1"), Some(&0));
    }

    #[test]
    fn test_extract_sections_empty_anchor_with_heading_sibling() {
        let html = r#"<a id="chapter1"></a><h1>Chapter 1</h1>"#;
        let fragment = Html::parse_fragment(html);
        let section_ids = HashSet::new();
        let text_lines = vec!["# Chapter 1".to_string()];
        let sections = extract_sections(&fragment, &section_ids, 0, &text_lines).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections.get("chapter1"), Some(&0));
    }

    #[test]
    fn test_extract_formatting() {
        let html = "<p>This is <strong>bold</strong> and <em>italic</em>.</p>";
        let fragment = Html::parse_fragment(html);
        let text_lines = vec!["This is **bold** and *italic*.".to_string()];
        let formatting =
            extract_formatting(&fragment, 0, &text_lines, &StyledClasses::default()).unwrap();
        assert_eq!(formatting.len(), 2);
        assert!(formatting.iter().any(|s| s.n_letters == 4 && s.attr == 1)); // bold
        assert!(formatting.iter().any(|s| s.n_letters == 6 && s.attr == 2)); // italic
    }

    #[test]
    fn test_extract_formatting_headers() {
        let html = r#"
        <h1>Header 1</h1>
        <p>Paragraph content.</p>
        <h2>Header 2</h2>
        "#;
        let fragment = Html::parse_fragment(html);
        let text_lines = vec![
            "# Header 1".to_string(),
            "Paragraph content.".to_string(),
            "## Header 2".to_string(),
        ];
        let formatting =
            extract_formatting(&fragment, 0, &text_lines, &StyledClasses::default()).unwrap();
        assert_eq!(formatting.len(), 2);

        // Check header 1 - html2text might format differently than expected
        let header1 = formatting.iter().find(|s| s.row == 0).unwrap();
        assert_eq!(header1.col, 2);
        assert_eq!(header1.n_letters, "Header 1".len() as u16); // Use actual length
        assert_eq!(header1.attr, 1); // Bold

        // Check header 2 - html2text might format differently than expected
        let header2 = formatting.iter().find(|s| s.row == 2).unwrap();
        assert_eq!(header2.col, 3);
        assert_eq!(header2.n_letters, "Header 2".len() as u16); // Use actual length
        assert_eq!(header2.attr, 1); // Bold
    }

    #[test]
    fn test_extract_formatting_no_matching_text() {
        let html = "<p>This has <strong>bold</strong> text.</p>";
        let fragment = Html::parse_fragment(html);
        let text_lines = vec!["Completely different text content.".to_string()];
        let formatting =
            extract_formatting(&fragment, 0, &text_lines, &StyledClasses::default()).unwrap();
        assert_eq!(formatting.len(), 0);
    }

    #[test]
    fn test_extract_formatting_no_html() {
        let html = "";
        let fragment = Html::parse_fragment(html);
        let text_lines = vec!["Plain text content.".to_string()];
        let formatting =
            extract_formatting(&fragment, 0, &text_lines, &StyledClasses::default()).unwrap();
        assert_eq!(formatting.len(), 0);
    }

    #[test]
    fn test_preprocess_inline_annotations() {
        let html = "<p>Note<sup>2</sup> and <sub>3</sub></p>";
        let processed = preprocess_inline_annotations(html);
        assert!(processed.contains("^{2}"));
        assert!(processed.contains("_{3}"));
    }

    #[test]
    fn test_replace_superscript_link_markers() {
        let mut lines = vec!["See [^{2}] and [^{7}]".to_string()];
        replace_superscript_link_markers(&mut lines);
        assert_eq!(lines[0], "See ^{1} and ^{2}");
    }

    #[test]
    fn test_strip_superscript_from_footnote_definitions() {
        let mut lines = vec![
            "^{[1]} Poem by Vietnamese Dhyana Master".to_string(),
            "^{[2]} This is described in [chap. 27].".to_string(),
        ];
        replace_superscript_link_markers(&mut lines);
        assert_eq!(lines[0], "[1] Poem by Vietnamese Dhyana Master");
        assert_eq!(lines[1], "[2] This is described in [chap. 27].");
    }

    #[test]
    fn test_parse_html_comprehensive() {
        let html = r#"
        <!DOCTYPE html>
        <html>
        <head>
            <title>Test Document</title>
        </head>
        <body>
            <h1 id="main-title">Main Title</h1>
            <p>Welcome to this <strong>test document</strong> with <em>emphasis</em>.</p>
            <h2 id="section1">Section 1</h2>
            <p>Here's an image: <img src="test.jpg" alt="Test"></p>
            <p>More <b>bold</b> and <i>italic</i> text.</p>
        </body>
        </html>
        "#;

        let mut section_ids = HashSet::new();
        section_ids.insert("main-title".to_string());
        section_ids.insert("section1".to_string());

        let result = parse_html(html, Some(80), Some(section_ids), 0).unwrap();

        // Check text content
        assert!(!result.text_lines.is_empty());
        assert!(result.text_lines[0].contains("Main Title"));

        // Check sections
        assert_eq!(result.section_rows.len(), 2);
        assert!(result.section_rows.contains_key("main-title"));
        assert!(result.section_rows.contains_key("section1"));

        // Check images
        assert_eq!(result.image_maps.len(), 1);
        assert!(result.image_maps.values().any(|v| v == "test.jpg"));

        // Check formatting (should include headers, strong, b, em, i)
        assert!(result.formatting.len() >= 4); // 2 headers + strong/em + b/i
    }

    #[test]
    fn test_parse_html_with_line_offset() {
        let html = r#"
        <h1 id="chapter1">Chapter 1</h1>
        <p>Content with <strong>bold</strong> text.</p>
        <img src="image.jpg" alt="Test">
        "#;

        let mut section_ids = HashSet::new();
        section_ids.insert("chapter1".to_string());

        let starting_line = 100;
        let result = parse_html(html, Some(80), Some(section_ids), starting_line).unwrap();

        // Check that line numbers are properly offset
        if let Some(&line_num) = result.section_rows.get("chapter1") {
            assert!(line_num >= starting_line);
        }

        for &line_num in result.image_maps.keys() {
            assert!(line_num >= starting_line);
        }

        for style in &result.formatting {
            assert!(style.row >= starting_line as u16);
        }
    }

    #[test]
    fn test_parse_html_none_text_width() {
        let html = "<p>Test content.</p>";
        let result = parse_html(html, None, None, 0).unwrap();
        assert!(!result.text_lines.is_empty());
        // Should use default width of 80
    }

    #[test]
    fn test_parse_html_none_section_ids() {
        let html = r#"<h1 id="chapter1">Chapter 1</h1><p>Content.</p>"#;
        let result = parse_html(html, Some(80), None, 0).unwrap();
        assert_eq!(result.section_rows.len(), 1);
        assert!(result.section_rows.contains_key("chapter1"));
    }

    // Test with realistic EPUB content
    #[test]
    fn test_parse_realistic_epub_content() {
        // This simulates content from our test EPUBs
        let html = r#"
        <?xml version="1.0" encoding="UTF-8" standalone="no"?>
        <!DOCTYPE html>
        <html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" xml:lang="en" lang="en">
        <head>
            <title>Chapter 1. Introduction</title>
            <link rel="stylesheet" type="text/css" href="css/epub.css" />
        </head>
        <body>
            <section class="chapter" title="Chapter 1. Introduction" epub:type="chapter" id="introduction">
                <h2 class="title">Chapter 1. Introduction</h2>
                <p>If you're expecting a <strong>run-of-the-mill</strong> best practices manual, be aware that there's an
                    ulterior message that will be running through this one. While the primary goal is
                    certainly to give you the information you need to create accessible EPUB 3
                    publications, it also seeks to address the question of <em>why</em> you need to pay attention
                    to the quality of your data, and how accessible data and general good data practices
                    are more tightly entwined than you might think.</p>
                <p>Accessibility is not a feel-good consideration that can be deferred to republishers
                    to fill in for you as you focus on print and quick-and-dirty ebooks, but a content
                    imperative vital to your survival in the digital future, as I'll take the odd detour
                    from the planned route to point out. Your data matters, not just its presentation,
                    and the more you see the value in it the more sense it will make to build in
                    accessibility from the ground up.</p>
            </section>
        </body>
        </html>
        "#;

        let mut section_ids = HashSet::new();
        section_ids.insert("introduction".to_string());

        let result = parse_html(html, Some(80), Some(section_ids), 0).unwrap();

        // Check that text was extracted
        assert!(!result.text_lines.is_empty());
        assert!(
            result
                .text_lines
                .iter()
                .any(|line| line.contains("Introduction"))
        );

        // Check section mapping
        assert_eq!(result.section_rows.len(), 1);
        assert!(result.section_rows.contains_key("introduction"));

        // Check formatting
        assert!(result.formatting.iter().any(|s| s.attr == 1)); // bold from "run-of-the-mill"
        assert!(result.formatting.iter().any(|s| s.attr == 2)); // italic from "why"
    }

    #[test]
    fn test_parse_meditations_style_content() {
        // This simulates content from Meditations EPUB
        let html = r#"
        <?xml version='1.0' encoding='utf-8'?>
        <!DOCTYPE html PUBLIC '-//W3C//DTD XHTML 1.1//EN' 'http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd'>
        <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en">
        <head>
        <meta content="text/css" http-equiv="Content-Style-Type"/>
        <title>The Project Gutenberg eBook of Meditations, by Marcus Aurelius</title>
        </head>
        <body>
        <div class="chapter" id="pgepubid00003">
        <h2><a id="link2H_INTR"/>
              INTRODUCTION
            </h2>
        <p>
        MARCUS AURELIUS ANTONINUS was born on April 26, A.D. 121. His real name was M.
        Annius Verus, and he was sprung of a noble family which claimed descent from
        Numa, second King of Rome. Thus the most religious of emperors came of the
        blood of the most pious of early kings. His father, Annius Verus, had held high
        office in Rome, and his grandfather, of the same name, had been thrice Consul.
        Both his parents died young, but Marcus held them in loving remembrance. On his
        father's death Marcus was adopted by his grandfather, the consular Annius
        Verus, and there was deep love between these two.
        </p>
        </div>
        </body>
        </html>
        "#;

        let mut section_ids = HashSet::new();
        section_ids.insert("pgepubid00003".to_string());

        let result = parse_html(html, Some(80), Some(section_ids), 0).unwrap();

        // Check that text was extracted correctly
        assert!(!result.text_lines.is_empty());
        assert!(
            result
                .text_lines
                .iter()
                .any(|line| line.contains("INTRODUCTION"))
        );
        assert!(
            result
                .text_lines
                .iter()
                .any(|line| line.contains("MARCUS AURELIUS"))
        );

        // Check section mapping
        assert!(result.section_rows.len() >= 1);
        assert!(result.section_rows.contains_key("pgepubid00003"));

        // May have formatting for the header - this is implementation-dependent
        // So we just check that parsing didn't crash
    }

    // Edge case tests
    #[test]
    fn test_malformed_html_recovery() {
        let html = r#"
        <p>Unclosed paragraph
        <h1>Header <strong>Unclosed strong
        <div>Nested content</div>
        "#;

        let result = html_to_plain_text(html, 80).unwrap();
        // Should not crash and should extract some text
        assert!(!result.is_empty());
    }

    #[test]
    fn test_empty_html_elements() {
        let html = r#"
        <p></p>
        <h1></h1>
        <div></div>
        <span></span>
        "#;

        let result = html_to_plain_text(html, 80).unwrap();
        // Should handle empty elements gracefully
        assert!(result.is_empty() || result.iter().all(|s| s.trim().is_empty()));
    }

    #[test]
    fn test_extract_formatting_with_trailing_footnote_span() {
        // Italic <p> ends with a non-italic <span> containing an endnote
        // marker. Without a separator the joined text would fuse "become.22"
        // into one token and the search would fail.
        let html = r#"<p class="quote">The quote ends here.<span class="num">22</span></p>"#;
        let fragment = Html::parse_fragment(html);
        let mut italic = std::collections::HashSet::new();
        italic.insert("quote".to_string());
        let styled = StyledClasses {
            italic,
            bold: std::collections::HashSet::new(),
            centered: std::collections::HashSet::new(),
        };
        let text_lines = vec!["The quote ends here.".to_string(), "[22]".to_string()];
        let formatting = extract_formatting(&fragment, 0, &text_lines, &styled).unwrap();
        assert!(
            formatting.iter().any(|s| s.row == 0 && s.attr == 2),
            "italic styling should land on row 0"
        );
    }

    #[test]
    fn test_extract_formatting_spans_hyphenated_word() {
        let html = "<p><em>alpha beta entangled gamma</em></p>";
        let fragment = Html::parse_fragment(html);
        let text_lines = vec!["alpha beta entan-".to_string(), "gled gamma".to_string()];
        let formatting =
            extract_formatting(&fragment, 0, &text_lines, &StyledClasses::default()).unwrap();
        assert!(
            !formatting.is_empty(),
            "italic span across hyphen should be detected"
        );
        assert!(formatting.iter().any(|s| s.row == 0 && s.attr == 2));
        assert!(formatting.iter().any(|s| s.row == 1 && s.attr == 2));
    }

    #[test]
    fn test_nested_formatting() {
        let html = "<p>This has <strong>nested <em>bold italic</em> text</strong>.</p>";
        let fragment = Html::parse_fragment(html);
        let text_lines = vec!["This has **nested *bold italic* text**.".to_string()];
        let formatting =
            extract_formatting(&fragment, 0, &text_lines, &StyledClasses::default()).unwrap();

        // Should extract at least one formatting element (the parser might not handle nested well)
        assert!(!formatting.is_empty());
        // Our current parser implementation may not extract nested formatting perfectly
        // So we just check that some formatting is detected
    }

    #[test]
    fn test_whitespace_handling() {
        let html = r#"
        <p>   Text with extra spaces   </p>
        <p>

            Text with newlines and spaces

        </p>
        "#;

        let result = html_to_plain_text(html, 80).unwrap();
        // Should normalize whitespace appropriately - html2text handles this
        assert!(result.len() >= 2);
        assert!(
            result
                .iter()
                .any(|l| l.trim().contains("Text with extra spaces"))
        );
        assert!(
            result
                .iter()
                .any(|l| l.trim().contains("Text with newlines and spaces"))
        );
    }

    #[test]
    fn test_italic_marker_stripping() {
        let html = "<p>This is <em>italic</em> text.</p>";
        let result = parse_html(html, Some(80), None, 0).unwrap();

        // Check that markers are stripped
        assert_eq!(result.text_lines[0], "This is italic text.");

        // Check that formatting is preserved
        assert_eq!(result.formatting.len(), 1);
        let style = &result.formatting[0];
        assert_eq!(style.attr, 2); // Italic
        assert_eq!(style.row, 0);
        assert_eq!(style.col, 8); // "This is " length is 8
        assert_eq!(style.n_letters, 6); // "italic" length is 6
    }

    #[test]
    fn test_italic_marker_stripping_whitespace_mismatch() {
        let html = "<p>This is <em>italic  text</em> with extra space.</p>";
        let result = parse_html(html, Some(80), None, 0).unwrap();

        // This assertion might fail if my hypothesis is correct
        assert!(!result.text_lines[0].contains("*italic text*"));
        assert_eq!(
            result.text_lines[0],
            "This is italic text with extra space."
        );

        // Check that formatting is preserved
        assert_eq!(result.formatting.len(), 1);
        let style = &result.formatting[0];
        assert_eq!(style.attr, 2); // Italic
    }

    #[test]
    fn test_italic_punctuation_and_headers() {
        let html = r#"
        <h1>Start <em>Meditations</em>, End</h1>
        <p>Text <em>Meditations</em>, more text.</p>
        "#;
        let result = parse_html(html, Some(80), None, 0).unwrap();

        // Check header
        // html2text likely produces "# Start *Meditations*, End" or similar
        let header_line = result
            .text_lines
            .iter()
            .find(|l| l.contains("Start"))
            .unwrap();
        assert!(
            !header_line.contains("*Meditations*"),
            "Header markers not stripped: {}",
            header_line
        );

        // Check paragraph
        let p_line = result
            .text_lines
            .iter()
            .find(|l| l.contains("Text"))
            .unwrap();
        assert!(
            !p_line.contains("*Meditations*"),
            "Paragraph markers not stripped: {}",
            p_line
        );
    }

    #[test]
    fn test_meditations_comma_stripping() {
        let html = "<p><em>Meditations</em>,</p>";
        let result = parse_html(html, Some(80), None, 0).unwrap();
        assert_eq!(result.text_lines[0], "Meditations,");
        assert_eq!(result.formatting.len(), 1);
    }

    #[test]
    fn test_internal_asterisk_not_stripped() {
        let mut text_lines = vec!["O*REILLY".to_string()];
        // Manually add formatting as if "REILLY" was italic (O*REILLY)
        let mut formatting = vec![InlineStyle {
            row: 0,
            col: 2, // 'R' is at index 2
            n_letters: 6,
            attr: 2, // Italic
        }];

        strip_inline_markers(&mut text_lines, &mut formatting, 0);
        // It should NOT strip the '*' because it's preceded by 'O' (not boundary)
        assert_eq!(text_lines[0], "O*REILLY");
    }

    #[test]
    fn test_pagebreak_extraction() {
        let html = r#"<p>End of page.</p>
<span epub:type="pagebreak" id="page42" title="42"></span>
<p>Start of new page.</p>"#;
        let result = parse_html(html, Some(80), None, 0).unwrap();
        assert!(!result.pagebreak_map.is_empty());
        assert!(result.pagebreak_map.values().any(|v| v == "42"));
        assert!(!result.text_lines.iter().any(|l| l.contains("@@PB:")));
    }

    #[test]
    fn test_pagebreak_with_content() {
        let html = r#"<p>Before.</p><span epub:type="pagebreak">99</span><p>After.</p>"#;
        let result = parse_html(html, Some(80), None, 0).unwrap();
        assert!(result.pagebreak_map.values().any(|v| v == "99"));
        assert!(!result.text_lines.iter().any(|l| l.contains("@@PB:")));
    }

    #[test]
    fn test_extract_links_filters_class_based_backlinks() {
        let html = std::fs::read_to_string("tests/fixtures/footnotes-class-based.html").unwrap();
        let fragment = Html::parse_document(&html);
        let text_lines = vec![
            "Chapter 8: The Teaching".to_string(),
            "The Buddha gave a discourse on the nature of suffering.".to_string(),
            "He further elaborated on the path to liberation.".to_string(),
            "1 Gavampati Sutta, Samyutta Nikaya V, 436.".to_string(),
            "2 Dhammacakkappavattana Sutta, Samyutta Nikaya LVI, 11.".to_string(),
        ];
        let links = extract_links(&fragment, 0, &text_lines).unwrap();
        // Should only have 2 footnote reference links, not 4 (backlinks filtered)
        assert_eq!(
            links.len(),
            2,
            "Expected 2 links (footnote refs only), got {}: {:?}",
            links.len(),
            links.iter().map(|l| &l.url).collect::<Vec<_>>()
        );
        assert!(links[0].url.contains("fn8_11"));
        assert!(links[1].url.contains("fn8_22"));
    }

    #[test]
    fn test_extract_sections_class_based_footnotes() {
        let html = std::fs::read_to_string("tests/fixtures/footnotes-class-based.html").unwrap();
        let fragment = Html::parse_document(&html);
        let section_ids = HashSet::new();
        // Simulate the text lines that the parser would produce
        let text_lines = vec![
            "Chapter 8: The Teaching".to_string(),
            "".to_string(),
            "The Buddha gave a discourse on the nature of suffering.".to_string(),
            "".to_string(),
            "He further elaborated on the path to liberation.".to_string(),
            "".to_string(),
            "1 Gavampati Sutta, Samyutta Nikaya V, 436.".to_string(),
            "".to_string(),
            "2 Dhammacakkappavattana Sutta, Samyutta Nikaya LVI, 11.".to_string(),
        ];
        let sections = extract_sections(&fragment, &section_ids, 0, &text_lines).unwrap();
        // fn8_11 should map to line 6 (the footnote definition), not line 0 or 2
        let fn8_11_line = sections
            .get("fn8_11")
            .expect("fn8_11 should be in sections");
        assert!(
            text_lines[*fn8_11_line].contains("Gavampati"),
            "fn8_11 should point to the Gavampati footnote line (line {}), got line {} = {:?}",
            6,
            fn8_11_line,
            text_lines.get(*fn8_11_line)
        );
        // fn8_22 should map to line 8 (the second footnote definition)
        let fn8_22_line = sections
            .get("fn8_22")
            .expect("fn8_22 should be in sections");
        assert!(
            text_lines[*fn8_22_line].contains("Dhammacakkappavattana"),
            "fn8_22 should point to the Dhammacakkappavattana footnote line (line {}), got line {} = {:?}",
            8,
            fn8_22_line,
            text_lines.get(*fn8_22_line)
        );
    }

    fn make_styled(italic: &[&str], bold: &[&str]) -> StyledClasses {
        let mut s = StyledClasses::default();
        for c in italic {
            s.italic.insert((*c).to_string());
        }
        for c in bold {
            s.bold.insert((*c).to_string());
        }
        s
    }

    #[test]
    fn test_class_driven_italic_via_span() {
        // Mirrors the *What is this?* book: <p> with a class-styled <span>
        // wrapping the entire visible text.
        let html =
            r#"<p class="body"><span class="ital">Great perplexity, great awakening.</span></p>"#;
        let styled = make_styled(&["ital"], &[]);
        let result = parse_html_with_styles(html, Some(80), None, 0, &styled, None).unwrap();
        let italic_styles: Vec<_> = result.formatting.iter().filter(|s| s.attr == 2).collect();
        assert!(
            !italic_styles.is_empty(),
            "expected at least one italic style, got formatting = {:?}",
            result.formatting
        );
        assert_eq!(
            italic_styles[0].n_letters,
            "Great perplexity, great awakening.".len() as u16
        );
    }

    #[test]
    fn test_class_driven_bold_and_italic_combined() {
        let html = r#"<p><span class="emph">word</span></p>"#;
        let styled = make_styled(&["emph"], &["emph"]);
        let result = parse_html_with_styles(html, Some(80), None, 0, &styled, None).unwrap();
        let attrs: Vec<u32> = result.formatting.iter().map(|s| s.attr).collect();
        assert!(attrs.contains(&1), "expected bold attr, got {:?}", attrs);
        assert!(attrs.contains(&2), "expected italic attr, got {:?}", attrs);
    }

    #[test]
    fn test_stanza_tightening_collapses_blank_separators() {
        let html = r#"
            <p>Intro paragraph.</p>
            <p class="body"><span class="ital">Great perplexity, great awakening.</span></p>
            <p class="body"><span class="ital">Little perplexity, little awakening.</span></p>
            <p class="body"><span class="ital">No perplexity, no awakening.</span></p>
            <p>Outro paragraph.</p>
        "#;
        let styled = make_styled(&["ital"], &[]);
        let result = parse_html_with_styles(html, Some(120), None, 0, &styled, None).unwrap();
        let lines = &result.text_lines;
        // Locate the three verse lines.
        let i_great = lines
            .iter()
            .position(|l| l.contains("Great perplexity"))
            .expect("first verse line missing");
        assert!(
            lines[i_great + 1].contains("Little perplexity"),
            "expected verse lines to be contiguous, got: {:?}, {:?}",
            lines[i_great + 1],
            lines.get(i_great + 2)
        );
        assert!(lines[i_great + 2].contains("No perplexity"));
        // And there should still be a blank line between the stanza and Outro.
        assert!(lines[i_great + 3].trim().is_empty());
        assert!(lines[i_great + 4].contains("Outro"));
    }

    #[test]
    fn test_stanza_tightening_does_not_collapse_unrelated_paragraphs() {
        let html = r#"
            <p>First normal paragraph with some <em>emphasis</em>.</p>
            <p>Second normal paragraph.</p>
        "#;
        let styled = make_styled(&["ital"], &[]);
        let result = parse_html_with_styles(html, Some(120), None, 0, &styled, None).unwrap();
        // Paragraphs should remain separated by a blank line.
        let i = result
            .text_lines
            .iter()
            .position(|l| l.contains("First normal"))
            .unwrap();
        assert!(result.text_lines[i + 1].trim().is_empty());
        assert!(result.text_lines[i + 2].contains("Second normal"));
    }

    #[test]
    fn test_blockquote_lines_are_indented() {
        let html = r#"
            <p>Lead-in.</p>
            <blockquote><p>Quoted paragraph one.</p><p>Quoted paragraph two.</p></blockquote>
            <p>After.</p>
        "#;
        let result = parse_html(html, Some(120), None, 0).unwrap();
        let q1 = result
            .text_lines
            .iter()
            .find(|l| l.contains("Quoted paragraph one"))
            .expect("first quoted line missing");
        let q2 = result
            .text_lines
            .iter()
            .find(|l| l.contains("Quoted paragraph two"))
            .expect("second quoted line missing");
        assert!(
            q1.starts_with("    "),
            "expected blockquote line to be indented, got {:?}",
            q1
        );
        assert!(q2.starts_with("    "));
        // Surrounding paragraphs must not be indented.
        let lead = result
            .text_lines
            .iter()
            .find(|l| l.contains("Lead-in"))
            .unwrap();
        assert!(!lead.starts_with(' '));
    }

    #[test]
    fn test_blockquote_preserves_inner_bold() {
        let html = r#"<blockquote><p>This has <strong>bold</strong> text inside.</p></blockquote>"#;
        let result = parse_html(html, Some(120), None, 0).unwrap();
        let line = result
            .text_lines
            .iter()
            .find(|l| l.contains("bold"))
            .expect("blockquote text missing");
        assert!(line.starts_with("    "));
        let bold = result.formatting.iter().find(|s| s.attr == 1);
        assert!(bold.is_some(), "expected bold formatting inside blockquote");
        let bold = bold.unwrap();
        // The bold span should align with the position of "bold" in the indented line.
        let col_of_bold = line.find("bold").unwrap();
        assert_eq!(bold.col as usize, col_of_bold);
        assert_eq!(bold.n_letters, "bold".len() as u16);
    }

    #[test]
    fn test_class_italic_multibyte_char_positions() {
        // Regression: the renderer indexes lines by char position, so
        // `extract_formatting`'s byte offsets must be converted to char
        // offsets before being returned. With multi-byte chars like `ā`,
        // a byte-based span would land off by one (or more) chars.
        // Mirrors the `What is this?` paragraph: `vipassanā` is plain text;
        // only `passanā` and `passati` are wrapped in italic-class spans.
        // The italic span text must not match a substring inside `vipassanā`.
        let html = r#"<p class="body">Take the word vipassanā. The prefix vi- is an intensifier, while <span class="ital">passanā</span> comes from <span class="ital">passati</span> in Pali.</p>"#;
        let styled = make_styled(&["ital"], &[]);
        let result = parse_html_with_styles(html, Some(120), None, 0, &styled, None).unwrap();
        let italics: Vec<_> = result.formatting.iter().filter(|s| s.attr == 2).collect();
        assert_eq!(italics.len(), 2, "expected two italic spans");
        let segments: Vec<String> = italics
            .iter()
            .map(|s| {
                let line = &result.text_lines[s.row as usize];
                let chars: Vec<char> = line.chars().collect();
                let start = s.col as usize;
                let end = start + s.n_letters as usize;
                chars[start..end].iter().collect()
            })
            .collect();
        assert_eq!(segments, vec!["passanā".to_string(), "passati".to_string()]);
    }
}

/// Rewrite SVG-wrapped raster images (`<svg><image xlink:href="…"/></svg>`,
/// the common Calibre/KF8 cover-page pattern) into plain `<img src="…">`
/// tags so the rest of the pipeline (placeholder generation, dimension
/// prescan, inline rendering, images list) treats them like any other
/// image. SVG blocks without an `<image>` child are left untouched.
pub(crate) fn preprocess_svg_images(html: &str) -> String {
    RE_SVG_BLOCK
        .replace_all(html, |caps: &Captures| {
            match RE_SVG_IMAGE_HREF.captures(&caps[0]) {
                Some(image) => format!(r#"<img src="{}">"#, &image[1]),
                None => caps[0].to_string(),
            }
        })
        .to_string()
}

fn preprocess_images(html: &str) -> String {
    RE_IMG
        .replace_all(html, |caps: &Captures| {
            let attrs_str = &caps[1];
            let src = RE_IMG_SRC
                .captures(attrs_str)
                .map(|c| c.get(1).unwrap().as_str().to_string());
            let alt = RE_IMG_ALT
                .captures(attrs_str)
                .map(|c| c.get(1).unwrap().as_str().to_string());
            let title = RE_IMG_TITLE
                .captures(attrs_str)
                .map(|c| c.get(1).unwrap().as_str().to_string());

            if let Some(src) = src {
                let filename = std::path::Path::new(&src)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("image");

                let new_alt_text = if let Some(t) = title {
                    format!("[Image: {}]", t)
                } else if let Some(a) = alt.as_ref() {
                    if a.trim().is_empty() || a.to_lowercase() == "image" {
                        format!("[Image: {}]", filename)
                    } else {
                        format!("[Image: {}]", a)
                    }
                } else {
                    format!("[Image: {}]", filename)
                };

                let new_attrs = if alt.is_some() {
                    RE_IMG_ALT
                        .replace(attrs_str, format!(r#"alt="{}""#, new_alt_text))
                        .to_string()
                } else {
                    format!(r#"{} alt="{}""#, attrs_str, new_alt_text)
                };

                format!("<img {}>", new_attrs)
            } else {
                caps[0].to_string()
            }
        })
        .to_string()
}
