pub mod bookmarks;
pub mod dictionary;
pub mod help;
pub mod images;
pub mod library;
pub mod links;
pub mod metadata;
pub mod search;
pub mod settings;
pub mod statistics;
pub mod toc;

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::layout::Rect;

/// Fuzzy-match `query` against `items`, returning the original indices of
/// matching items ordered by descending match score. An empty query matches
/// everything in the original order.
pub fn fuzzy_filter_indices(query: &str, items: &[impl AsRef<str>]) -> Vec<usize> {
    if query.is_empty() {
        return (0..items.len()).collect();
    }
    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| {
            let haystack = Utf32Str::new(item.as_ref(), &mut buf);
            pattern.score(haystack, &mut matcher).map(|score| (score, i))
        })
        .collect();
    // Sort by score descending; original order breaks ties (stable sort).
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, i)| i).collect()
}

/// Compute a centered popup area within the given area.
pub fn centered_popup_area(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let width = (area.width * width_percent) / 100;
    let height = (area.height * height_percent) / 100;
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;

    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_filter_empty_query_keeps_all_in_order() {
        let items = vec!["Chapter One", "Chapter Two", "Appendix"];
        assert_eq!(fuzzy_filter_indices("", &items), vec![0, 1, 2]);
    }

    #[test]
    fn fuzzy_filter_matches_subsequences_case_insensitively() {
        let items = vec!["Introduction", "Chapter One", "Chapter Two"];
        let indices = fuzzy_filter_indices("chp", &items);
        assert_eq!(indices, vec![1, 2]);
    }

    #[test]
    fn fuzzy_filter_ranks_better_matches_first() {
        // "map" is a scattered subsequence of the first item but an exact
        // match of the second, so the second must rank first.
        let items = vec!["made a plan", "map"];
        let indices = fuzzy_filter_indices("map", &items);
        assert_eq!(indices, vec![1, 0]);
    }

    #[test]
    fn fuzzy_filter_no_matches_returns_empty() {
        let items = vec!["Chapter One", "Chapter Two"];
        assert!(fuzzy_filter_indices("zzz", &items).is_empty());
    }
}
