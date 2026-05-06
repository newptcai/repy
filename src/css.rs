//! Lightweight CSS scanner.
//!
//! Builds a map of class names whose computed style is italic or bold. The goal
//! is to recover inline emphasis from books (e.g. InDesign/Calibre exports) that
//! style emphasis through a class on `<span>` rather than via `<em>` / `<strong>`.
//!
//! This is intentionally not a full CSS parser. It handles flat rule blocks of
//! the form `selector { decl; decl; ... }` with selectors of the shape
//! `tag.class`, `.class`, or `tag.class1.class2` (plus comma-separated lists of
//! those). Anything more elaborate — combinators, attribute selectors,
//! pseudo-classes, `@media` blocks, nested at-rules — is skipped safely.

use std::collections::HashSet;

#[derive(Debug, Default, Clone)]
pub struct StyledClasses {
    pub italic: HashSet<String>,
    pub bold: HashSet<String>,
}

impl StyledClasses {
    pub fn is_empty(&self) -> bool {
        self.italic.is_empty() && self.bold.is_empty()
    }
}

/// Scan the given stylesheet sources and return classes that produce italic or
/// bold rendering.
pub fn collect_styled_classes(stylesheets: &[&str]) -> StyledClasses {
    let mut out = StyledClasses::default();
    for sheet in stylesheets {
        scan_sheet(sheet, &mut out);
    }
    out
}

fn scan_sheet(input: &str, out: &mut StyledClasses) {
    let stripped = strip_comments(input);
    let bytes = stripped.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip whitespace.
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        // At-rules: skip the prelude and, if present, an entire {} block.
        if bytes[i] == b'@' {
            i = skip_at_rule(bytes, i);
            continue;
        }
        // Find the next `{` that opens a rule's block.
        let block_start = match find_unquoted(bytes, i, b'{') {
            Some(p) => p,
            None => break,
        };
        let selector = &stripped[i..block_start];
        let block_end = match find_matching_brace(bytes, block_start) {
            Some(p) => p,
            None => break,
        };
        let body = &stripped[block_start + 1..block_end];

        // Skip rules that contain nested blocks — we don't model nesting.
        if !body.contains('{') {
            apply_rule(selector, body, out);
        }
        i = block_end + 1;
    }
}

fn apply_rule(selector: &str, body: &str, out: &mut StyledClasses) {
    let italic = body_has_italic(body);
    let bold = body_has_bold(body);
    if !italic && !bold {
        return;
    }
    for sel in selector.split(',') {
        let sel = sel.trim();
        if sel.is_empty() {
            continue;
        }
        if let Some(classes) = extract_simple_classes(sel) {
            for c in classes {
                if italic {
                    out.italic.insert(c.clone());
                }
                if bold {
                    out.bold.insert(c);
                }
            }
        }
    }
}

/// Accept selectors of the shape `tag.class`, `.class`, or `tag.class1.class2`.
/// Returns None for selectors with combinators, attribute selectors, pseudo
/// classes, or descendant chains.
fn extract_simple_classes(sel: &str) -> Option<Vec<String>> {
    // Must contain at least one '.'
    if !sel.contains('.') {
        return None;
    }
    // Reject anything with combinators / pseudo / brackets / whitespace.
    for ch in sel.chars() {
        match ch {
            ' ' | '\t' | '\n' | '>' | '+' | '~' | '[' | ']' | ':' | '(' | ')' | '*' | '#' => {
                return None;
            }
            _ => {}
        }
    }
    // Split on '.', drop the first segment (tag, possibly empty), keep rest as class names.
    let mut parts = sel.split('.');
    let _tag = parts.next();
    let classes: Vec<String> = parts
        .filter(|p| !p.is_empty())
        .map(|p| p.to_string())
        .collect();
    if classes.is_empty() {
        None
    } else {
        Some(classes)
    }
}

fn body_has_italic(body: &str) -> bool {
    for decl in iter_declarations(body) {
        if let Some((prop, value)) = split_decl(decl) {
            if prop.eq_ignore_ascii_case("font-style") {
                let v = value.trim().to_ascii_lowercase();
                if v.starts_with("italic") || v.starts_with("oblique") {
                    return true;
                }
            } else if prop.eq_ignore_ascii_case("font") {
                // Shorthand: italic/oblique appears as a token.
                for tok in value.split_ascii_whitespace() {
                    let tok = tok.trim_end_matches(',');
                    if tok.eq_ignore_ascii_case("italic") || tok.eq_ignore_ascii_case("oblique") {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn body_has_bold(body: &str) -> bool {
    for decl in iter_declarations(body) {
        if let Some((prop, value)) = split_decl(decl) {
            if prop.eq_ignore_ascii_case("font-weight") {
                if weight_is_bold(value.trim()) {
                    return true;
                }
            } else if prop.eq_ignore_ascii_case("font") {
                for tok in value.split_ascii_whitespace() {
                    let tok = tok.trim_end_matches(',');
                    if weight_is_bold(tok) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn weight_is_bold(value: &str) -> bool {
    let v = value.trim().to_ascii_lowercase();
    if v == "bold" || v == "bolder" {
        return true;
    }
    if let Ok(n) = v.parse::<u32>() {
        return n >= 600;
    }
    false
}

fn iter_declarations(body: &str) -> impl Iterator<Item = &str> {
    body.split(';').map(str::trim).filter(|s| !s.is_empty())
}

fn split_decl(decl: &str) -> Option<(&str, &str)> {
    let colon = decl.find(':')?;
    let prop = decl[..colon].trim();
    let value = decl[colon + 1..].trim();
    if prop.is_empty() || value.is_empty() {
        None
    } else {
        Some((prop, value))
    }
}

fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            // Find closing */
            if let Some(end) = find_subsequence(bytes, i + 2, b"*/") {
                i = end + 2;
                continue;
            } else {
                break;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn find_subsequence(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || from + needle.len() > haystack.len() {
        return None;
    }
    (from..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
}

fn find_unquoted(bytes: &[u8], from: usize, target: u8) -> Option<usize> {
    let mut i = from;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => {
                if b == b'"' || b == b'\'' {
                    quote = Some(b);
                } else if b == target {
                    return Some(i);
                }
            }
        }
        i += 1;
    }
    None
}

fn find_matching_brace(bytes: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => {
                if b == b'"' || b == b'\'' {
                    quote = Some(b);
                } else if b == b'{' {
                    depth += 1;
                } else if b == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
            }
        }
        i += 1;
    }
    None
}

fn skip_at_rule(bytes: &[u8], from: usize) -> usize {
    // An at-rule prelude ends at either ';' (no block) or '{' (block follows).
    let mut i = from;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        match quote {
            Some(q) => {
                if b == q {
                    quote = None;
                }
            }
            None => {
                if b == b'"' || b == b'\'' {
                    quote = Some(b);
                } else if b == b';' {
                    return i + 1;
                } else if b == b'{' {
                    return find_matching_brace(bytes, i).map(|p| p + 1).unwrap_or(bytes.len());
                }
            }
        }
        i += 1;
    }
    bytes.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_italic_via_font_style() {
        let css = "span.CharOverride-14 { font-family: serif; font-style: italic; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.italic.contains("CharOverride-14"));
        assert!(s.bold.is_empty());
    }

    #[test]
    fn detects_oblique_as_italic() {
        let css = ".oblique-class { font-style: oblique; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.italic.contains("oblique-class"));
    }

    #[test]
    fn detects_bold_via_keyword() {
        let css = ".heavy { font-weight: bold; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.bold.contains("heavy"));
    }

    #[test]
    fn detects_bold_via_numeric_weight() {
        let css = ".w700 { font-weight: 700; } .w500 { font-weight: 500; } .w600 { font-weight: 600; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.bold.contains("w700"));
        assert!(s.bold.contains("w600"));
        assert!(!s.bold.contains("w500"));
    }

    #[test]
    fn handles_multi_selector_lists() {
        let css = ".a, .b , p.c { font-style: italic; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.italic.contains("a"));
        assert!(s.italic.contains("b"));
        assert!(s.italic.contains("c"));
    }

    #[test]
    fn skips_complex_selectors() {
        let css = "a > .child { font-style: italic; } .x:hover { font-style: italic; } [data-x].y { font-style: italic; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.italic.is_empty());
    }

    #[test]
    fn skips_at_media_safely() {
        let css = "@media print { .ignored { font-style: italic; } } .kept { font-style: italic; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.italic.contains("kept"));
        // Inside @media we don't recurse into nested blocks.
        assert!(!s.italic.contains("ignored"));
    }

    #[test]
    fn ignores_comments() {
        let css = "/* .commented { font-style: italic; } */ .real { font-style: italic; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.italic.contains("real"));
        assert!(!s.italic.contains("commented"));
    }

    #[test]
    fn font_shorthand_italic() {
        let css = ".sh { font: italic 12pt/14pt serif; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.italic.contains("sh"));
    }

    #[test]
    fn handles_multiple_classes_on_selector() {
        let css = "p.a.b { font-style: italic; }";
        let s = collect_styled_classes(&[css]);
        assert!(s.italic.contains("a"));
        assert!(s.italic.contains("b"));
    }

    #[test]
    fn empty_input_yields_empty() {
        let s = collect_styled_classes(&[]);
        assert!(s.is_empty());
    }
}
