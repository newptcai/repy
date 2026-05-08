use crate::ebook::Ebook;
use crate::models::{BookIdentity, Highlight, HighlightRange};
use eyre::Result;
use sha2::{Digest, Sha256};
use std::collections::HashMap;

pub const NORMALIZATION_VERSION: i64 = 1;
pub const COMMENT_MAX_CHARS: usize = 20_000;

#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    Resolved(Vec<HighlightRange>),
    Ambiguous,
    Unresolved,
}

#[derive(Debug, Clone)]
struct NormalizedChapter {
    text: String,
    chars: Vec<char>,
    char_map: Vec<Option<(usize, usize)>>,
}

pub fn normalize_text(text: &str) -> String {
    let mut out = String::new();
    let mut previous_space = true;
    for ch in text.chars() {
        if ch.is_whitespace() {
            if !previous_space {
                out.push(' ');
                previous_space = true;
            }
        } else {
            out.push(ch);
            previous_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

pub fn normalized_text_hash(lines: &[String], global_start_row: usize) -> String {
    let chapter = build_normalized_chapter(lines, global_start_row);
    sha256_hex(chapter.text.as_bytes())
}

pub fn derive_book_identity(ebook: &mut dyn Ebook) -> Result<BookIdentity> {
    let metadata = ebook.get_meta().clone();
    let mut hrefs = Vec::new();
    let mut fingerprints = Vec::new();
    let contents = ebook.contents().clone();

    for (index, content_id) in contents.iter().enumerate() {
        let href = ebook
            .spine_href(index)
            .unwrap_or_else(|| content_id.to_string());
        hrefs.push(href);
        let raw = ebook.get_raw_text(content_id)?;
        let prefix: String = raw.chars().take(2048).collect();
        let suffix_rev: String = raw.chars().rev().take(2048).collect();
        let suffix: String = suffix_rev.chars().rev().collect();
        fingerprints.push(format!("{}:{}:{}", raw.len(), prefix, suffix));
    }

    let spine_hrefs_hash = sha256_hex(hrefs.join("\n").as_bytes());
    let content_fingerprints_hash = sha256_hex(fingerprints.join("\n---\n").as_bytes());
    let identity_material = [
        metadata.identifier.clone().unwrap_or_default(),
        metadata.title.clone().unwrap_or_default(),
        metadata.creator.clone().unwrap_or_default(),
        spine_hrefs_hash.clone(),
        content_fingerprints_hash.clone(),
    ]
    .join("\n");
    let book_id = sha256_hex(identity_material.as_bytes());

    Ok(BookIdentity {
        book_id,
        identifier: metadata.identifier,
        title: metadata.title,
        creator: metadata.creator,
        spine_hrefs_hash,
        content_fingerprints_hash,
    })
}

pub fn anchor_from_selection(
    lines: &[String],
    global_start_row: usize,
    start: (usize, usize),
    end: (usize, usize),
) -> Option<(String, String, String, usize)> {
    let chapter = build_normalized_chapter(lines, global_start_row);
    if chapter.chars.is_empty() {
        return None;
    }

    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    let start_idx = chapter
        .char_map
        .iter()
        .position(|pos| pos.is_some_and(|(row, col)| (row, col) >= start))?;
    let end_idx = chapter
        .char_map
        .iter()
        .rposition(|pos| pos.is_some_and(|(row, col)| (row, col) <= end))?;
    if start_idx > end_idx {
        return None;
    }
    let exact: String = chapter.chars[start_idx..=end_idx].iter().collect();
    if exact.is_empty() {
        return None;
    }
    let approx_offset = start_idx;
    let prefix_start = start_idx.saturating_sub(32);
    let prefix: String = chapter.chars[prefix_start..start_idx].iter().collect();
    let suffix_end = (end_idx + 1 + 32).min(chapter.chars.len());
    let suffix: String = chapter.chars[end_idx + 1..suffix_end].iter().collect();
    Some((exact, prefix, suffix, approx_offset))
}

pub fn resolve_highlight(
    highlight_index: usize,
    highlight: &Highlight,
    lines: &[String],
    global_start_row: usize,
) -> Resolution {
    let chapter = build_normalized_chapter(lines, global_start_row);
    if chapter.text.is_empty() || highlight.exact.is_empty() {
        return Resolution::Unresolved;
    }
    let needle: Vec<char> = highlight.exact.chars().collect();
    let candidates = find_exact_candidates(&chapter.chars, &needle);
    if let Some(choice) = choose_near_candidate(&candidates, highlight.approx_offset, 200) {
        return ranges_for_candidate(highlight_index, &chapter, choice, needle.len());
    }
    if let Some(choice) = choose_scored_candidate(&chapter, &candidates, highlight) {
        return ranges_for_candidate(highlight_index, &chapter, choice, needle.len());
    }
    if !candidates.is_empty() {
        return Resolution::Ambiguous;
    }
    if let Some(choice) = fuzzy_candidate(&chapter.chars, &needle, highlight.approx_offset) {
        return ranges_for_candidate(highlight_index, &chapter, choice, needle.len());
    }
    Resolution::Unresolved
}

pub fn ranges_by_row_for_highlights(
    highlights: &[Highlight],
    chapter_lines: &[String],
    global_start_row: usize,
) -> (HashMap<usize, Vec<HighlightRange>>, Vec<(String, String)>) {
    let mut by_row: HashMap<usize, Vec<HighlightRange>> = HashMap::new();
    let mut statuses = Vec::new();
    for (idx, highlight) in highlights.iter().enumerate() {
        match resolve_highlight(idx, highlight, chapter_lines, global_start_row) {
            Resolution::Resolved(ranges) => {
                statuses.push((highlight.id.clone(), "resolved".to_string()));
                for range in ranges {
                    by_row.entry(range.row).or_default().push(range);
                }
            }
            Resolution::Ambiguous => statuses.push((highlight.id.clone(), "ambiguous".to_string())),
            Resolution::Unresolved => {
                statuses.push((highlight.id.clone(), "unresolved".to_string()))
            }
        }
    }
    for ranges in by_row.values_mut() {
        ranges.sort_by_key(|range| range.highlight_index);
    }
    (by_row, statuses)
}

fn build_normalized_chapter(lines: &[String], global_start_row: usize) -> NormalizedChapter {
    let mut text = String::new();
    let mut chars = Vec::new();
    let mut char_map = Vec::new();
    let mut previous_space = true;

    for (line_idx, line) in lines.iter().enumerate() {
        let row = global_start_row + line_idx;
        if line == crate::models::CHAPTER_BREAK_MARKER {
            continue;
        }
        if !previous_space && !line.is_empty() {
            text.push(' ');
            chars.push(' ');
            char_map.push(None);
            previous_space = true;
        }
        for (col, ch) in line.chars().enumerate() {
            if ch.is_whitespace() {
                if !previous_space {
                    text.push(' ');
                    chars.push(' ');
                    char_map.push(Some((row, col)));
                    previous_space = true;
                }
            } else {
                text.push(ch);
                chars.push(ch);
                char_map.push(Some((row, col)));
                previous_space = false;
            }
        }
    }

    if text.ends_with(' ') {
        text.pop();
        chars.pop();
        char_map.pop();
    }

    NormalizedChapter {
        text,
        chars,
        char_map,
    }
}

fn find_exact_candidates(haystack: &[char], needle: &[char]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(idx, window)| (window == needle).then_some(idx))
        .collect()
}

fn choose_near_candidate(candidates: &[usize], approx: usize, window: usize) -> Option<usize> {
    let mut near: Vec<usize> = candidates
        .iter()
        .copied()
        .filter(|idx| idx.abs_diff(approx) <= window)
        .collect();
    near.sort_by_key(|idx| idx.abs_diff(approx));
    match near.as_slice() {
        [only] => Some(*only),
        [first, second, ..] if first.abs_diff(approx) != second.abs_diff(approx) => Some(*first),
        _ => None,
    }
}

fn choose_scored_candidate(
    chapter: &NormalizedChapter,
    candidates: &[usize],
    highlight: &Highlight,
) -> Option<usize> {
    let needle_len = highlight.exact.chars().count();
    let prefix: Vec<char> = highlight.prefix.chars().collect();
    let suffix: Vec<char> = highlight.suffix.chars().collect();
    let mut scored = candidates
        .iter()
        .map(|&idx| {
            let prefix_score = common_suffix(&chapter.chars[..idx], &prefix);
            let suffix_score = common_prefix(&chapter.chars[idx + needle_len..], &suffix);
            let distance = idx.abs_diff(highlight.approx_offset);
            (
                prefix_score + suffix_score,
                std::cmp::Reverse(distance),
                idx,
            )
        })
        .collect::<Vec<_>>();
    scored.sort_by_key(|item| (item.0, item.1));
    scored.reverse();
    match scored.as_slice() {
        [(_, _, only)] => Some(*only),
        [(score_a, dist_a, first), (score_b, dist_b, _), ..]
            if score_a != score_b || dist_a != dist_b =>
        {
            Some(*first)
        }
        _ => None,
    }
}

fn fuzzy_candidate(haystack: &[char], needle: &[char], approx: usize) -> Option<usize> {
    if needle.is_empty() || haystack.is_empty() {
        return None;
    }
    let start = approx.saturating_sub(400);
    let end = (approx + needle.len() + 400).min(haystack.len());
    let max_distance = needle.len().min(5);
    let mut best: Option<(usize, usize)> = None;
    let min_len = needle.len().saturating_sub(5).max(1);
    let max_len = (needle.len() + 5).min(end.saturating_sub(start));
    for idx in start..end {
        for len in min_len..=max_len {
            if idx + len > end {
                break;
            }
            let distance = levenshtein(&haystack[idx..idx + len], needle);
            if distance <= max_distance {
                match best {
                    None => best = Some((distance + idx.abs_diff(approx), idx)),
                    Some((best_score, best_idx)) => {
                        let score = distance + idx.abs_diff(approx);
                        if score < best_score {
                            best = Some((score, idx));
                        } else if score == best_score && idx != best_idx {
                            return None;
                        }
                    }
                }
            }
        }
    }
    best.map(|(_, idx)| idx)
}

fn ranges_for_candidate(
    highlight_index: usize,
    chapter: &NormalizedChapter,
    start: usize,
    len: usize,
) -> Resolution {
    let end = start + len;
    if end > chapter.char_map.len() {
        return Resolution::Unresolved;
    }
    let mut ranges = Vec::new();
    let mut idx = start;
    while idx < end {
        let Some((row, start_col)) = chapter.char_map[idx] else {
            idx += 1;
            continue;
        };
        let mut end_col = start_col + 1;
        idx += 1;
        while idx < end {
            let Some((next_row, next_col)) = chapter.char_map[idx] else {
                break;
            };
            if next_row != row {
                break;
            }
            end_col = next_col + 1;
            idx += 1;
        }
        if start_col < end_col {
            ranges.push(HighlightRange {
                highlight_index,
                row,
                start_col,
                end_col,
            });
        }
    }
    if ranges.is_empty() {
        Resolution::Unresolved
    } else {
        Resolution::Resolved(ranges)
    }
}

fn common_prefix(a: &[char], b: &[char]) -> usize {
    a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
}

fn common_suffix(a: &[char], b: &[char]) -> usize {
    a.iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count()
}

fn levenshtein(a: &[char], b: &[char]) -> usize {
    let mut costs: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut last = i;
        costs[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let old = costs[j + 1];
            costs[j + 1] = if ca == cb {
                last
            } else {
                1 + last.min(costs[j]).min(costs[j + 1])
            };
            last = old;
        }
    }
    costs[b.len()]
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn highlight(exact: &str, prefix: &str, suffix: &str, approx_offset: usize) -> Highlight {
        Highlight {
            id: "h1".to_string(),
            book_id: "b1".to_string(),
            content_index: 0,
            spine_href: "c1.xhtml".to_string(),
            exact: exact.to_string(),
            prefix: prefix.to_string(),
            suffix: suffix.to_string(),
            approx_offset,
            normalization_version: NORMALIZATION_VERSION,
            color: "yellow".to_string(),
            comment: None,
            comment_format: "plain".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            resolution_status: "resolved".to_string(),
        }
    }

    #[test]
    fn selection_survives_wrapping_changes() {
        let lines = vec!["alpha beta".to_string(), "gamma delta".to_string()];
        let (exact, prefix, suffix, approx) =
            anchor_from_selection(&lines, 0, (0, 6), (1, 4)).unwrap();
        assert_eq!(exact, "beta gamma");

        let changed = vec!["alpha beta gamma".to_string(), "delta".to_string()];
        let h = highlight(&exact, &prefix, &suffix, approx);
        match resolve_highlight(0, &h, &changed, 0) {
            Resolution::Resolved(ranges) => {
                assert_eq!(ranges[0].row, 0);
                assert_eq!(ranges[0].start_col, 6);
            }
            other => panic!("unexpected resolution: {other:?}"),
        }
    }

    #[test]
    fn wrapped_line_highlight_draws_real_text_not_synthetic_spaces() {
        let lines = vec!["alpha beta".to_string(), "gamma delta".to_string()];
        let (exact, prefix, suffix, approx) =
            anchor_from_selection(&lines, 0, (0, 6), (1, 4)).unwrap();
        let h = highlight(&exact, &prefix, &suffix, approx);

        match resolve_highlight(0, &h, &lines, 0) {
            Resolution::Resolved(ranges) => {
                assert_eq!(ranges.len(), 2);
                assert_eq!(ranges[0].row, 0);
                assert_eq!(ranges[0].start_col, 6);
                assert_eq!(ranges[0].end_col, 10);
                assert_eq!(ranges[1].row, 1);
                assert_eq!(ranges[1].start_col, 0);
                assert_eq!(ranges[1].end_col, 5);
            }
            other => panic!("unexpected resolution: {other:?}"),
        }
    }

    #[test]
    fn repeated_tied_text_is_ambiguous() {
        let lines = vec!["same text same text".to_string()];
        let h = highlight("same text", "", "", 5);
        assert_eq!(resolve_highlight(0, &h, &lines, 0), Resolution::Ambiguous);
    }

    #[test]
    fn repeated_text_disambiguated_by_context() {
        let lines = vec!["same text same text".to_string()];
        let h = highlight("same text", "", " same", 0);
        match resolve_highlight(0, &h, &lines, 0) {
            Resolution::Resolved(ranges) => {
                assert_eq!(ranges[0].start_col, 0);
            }
            other => panic!("expected first occurrence resolved, got {other:?}"),
        }
        let h2 = highlight("same text", "text ", "", 10);
        match resolve_highlight(0, &h2, &lines, 0) {
            Resolution::Resolved(ranges) => {
                assert_eq!(ranges[0].start_col, 10);
            }
            other => panic!("expected second occurrence resolved, got {other:?}"),
        }
    }

    #[test]
    fn cjk_selection_resolves_at_correct_columns() {
        let lines = vec!["你好世界，这是一段中文。".to_string()];
        let (exact, prefix, suffix, approx) =
            anchor_from_selection(&lines, 0, (0, 5), (0, 11)).unwrap();
        let h = highlight(&exact, &prefix, &suffix, approx);
        match resolve_highlight(0, &h, &lines, 0) {
            Resolution::Resolved(ranges) => {
                assert_eq!(ranges.len(), 1);
                assert_eq!(ranges[0].row, 0);
                assert_eq!(ranges[0].start_col, 5);
                assert_eq!(ranges[0].end_col, 12);
            }
            other => panic!("unexpected resolution: {other:?}"),
        }
    }

    #[test]
    fn whitespace_only_edits_resolve() {
        let original = vec!["The quick brown fox".to_string()];
        let (exact, prefix, suffix, approx) =
            anchor_from_selection(&original, 0, (0, 4), (0, 14)).unwrap();
        assert_eq!(exact, "quick brown");
        let edited = vec![
            "The   quick".to_string(),
            "brown".to_string(),
            "fox".to_string(),
        ];
        let h = highlight(&exact, &prefix, &suffix, approx);
        assert!(matches!(
            resolve_highlight(0, &h, &edited, 0),
            Resolution::Resolved(_)
        ));
    }

    #[test]
    fn truly_missing_text_is_unresolved() {
        let lines = vec!["wholly different content here".to_string()];
        let h = highlight("the original quote", "before ", " after", 50);
        assert_eq!(resolve_highlight(0, &h, &lines, 0), Resolution::Unresolved);
    }

    #[test]
    fn normalize_text_collapses_whitespace_and_preserves_unicode() {
        assert_eq!(normalize_text("a   b\n\nc"), "a b c");
        assert_eq!(normalize_text("  你好  世界  "), "你好 世界");
        assert_eq!(normalize_text(""), "");
        assert_eq!(normalize_text("   "), "");
    }
}
