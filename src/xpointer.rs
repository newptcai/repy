//! Best-effort reader for the CREngine XPointers KOReader stores in a kosync
//! `progress` field, e.g. `/body/DocFragment[14]/body/div/p[1]/text().0`.
//!
//! repy only ever *reads* these on pull — it cannot generate a CREngine
//! XPointer, so it never pushes one. Reading is enough to pin the exact
//! chapter (`DocFragment[N]`, the 1-based spine index) and to place the reader
//! within that chapter far more precisely than the drifting global percentage.
//!
//! The within-chapter resolution is deliberately tolerant: crengine normalizes
//! the DOM (autoboxing, whitespace handling) in ways repy's parser does not, so
//! an element step can miss on complex markup. Callers treat a `None` (or an
//! implausible result) as "fall back to the percentage".

use scraper::{ElementRef, Html, Selector};

/// A parsed CREngine XPointer, reduced to what repy can act on.
#[derive(Debug, Clone, PartialEq)]
pub struct XPointer {
    /// 1-based spine document index from `DocFragment[N]`.
    pub doc_fragment: usize,
    /// Element steps from the chapter `<body>` to the target element, each a
    /// `(tag, 1-based index among same-named siblings)` pair.
    pub steps: Vec<(String, usize)>,
    /// Character offset into the target element's text (`text().OFFSET`).
    pub text_offset: usize,
}

/// Parse a CREngine XPointer string. Returns `None` when it does not look like
/// one (e.g. repy or another client stored a bare percentage there).
pub fn parse(progress: &str) -> Option<XPointer> {
    let progress = progress.trim();
    if progress.is_empty() {
        return None;
    }

    // Collect the tokens that follow the `DocFragment[N]` component.
    let mut doc_fragment = None;
    let mut rest: Vec<&str> = Vec::new();
    for token in progress.trim_start_matches('/').split('/') {
        if token.is_empty() {
            continue;
        }
        if doc_fragment.is_none() {
            if let Some(n) = parse_named_index(token, "DocFragment") {
                doc_fragment = Some(n);
            }
            continue;
        }
        rest.push(token);
    }
    let doc_fragment = doc_fragment?;

    // The first following token is the chapter's own `<body>`, our nav root.
    let mut rest = rest.as_slice();
    if let Some((first, tail)) = rest.split_first()
        && parse_step(first).is_some_and(|(tag, _)| tag == "body")
    {
        rest = tail;
    }

    // A trailing `text().OFFSET` component gives the intra-element offset.
    let mut text_offset = 0;
    if let Some((last, head)) = rest.split_last()
        && let Some(offset) = parse_text_offset(last)
    {
        text_offset = offset;
        rest = head;
    }

    let mut steps = Vec::with_capacity(rest.len());
    for token in rest {
        steps.push(parse_step(token)?);
    }

    Some(XPointer {
        doc_fragment,
        steps,
        text_offset,
    })
}

/// Resolve the XPointer against a chapter's raw XHTML to a fraction in
/// `[0.0, 1.0]` of the way through the chapter's text. `None` when an element
/// step cannot be followed.
pub fn resolve_fraction(chapter_html: &str, xp: &XPointer) -> Option<f64> {
    let document = Html::parse_document(chapter_html);
    let body_selector = Selector::parse("body").ok()?;
    let body = document.select(&body_selector).next()?;

    let mut current = body;
    for (tag, index) in &xp.steps {
        current = nth_child_element(current, tag, *index)?;
    }

    let total = text_len(body);
    if total == 0 {
        return Some(0.0);
    }
    let before = text_len_before(body, current);
    let offset = (before + xp.text_offset).min(total);
    Some(offset as f64 / total as f64)
}

/// The `k`-th (1-based) child element of `parent` named `tag`.
fn nth_child_element<'a>(parent: ElementRef<'a>, tag: &str, k: usize) -> Option<ElementRef<'a>> {
    parent
        .children()
        .filter_map(ElementRef::wrap)
        .filter(|element| element.value().name() == tag)
        .nth(k.saturating_sub(1))
}

/// Total character count of all text under `element`.
fn text_len(element: ElementRef) -> usize {
    element.text().map(|t| t.chars().count()).sum()
}

/// Character count of all text under `body` that precedes `target` in document
/// order.
fn text_len_before(body: ElementRef, target: ElementRef) -> usize {
    let target_id = target.id();
    let mut sum = 0;
    for node in body.descendants() {
        if node.id() == target_id {
            break;
        }
        if let Some(text) = node.value().as_text() {
            sum += text.chars().count();
        }
    }
    sum
}

/// `"name[k]"` -> `k`, matching only when `name` is exactly `expected`.
fn parse_named_index(token: &str, expected: &str) -> Option<usize> {
    let (name, index) = parse_step(token)?;
    (name == expected).then_some(index)
}

/// `"tag[k]"` -> `(tag, k)`; `"tag"` -> `(tag, 1)`.
fn parse_step(token: &str) -> Option<(String, usize)> {
    match token.find('[') {
        Some(open) => {
            let name = &token[..open];
            let close = token.find(']')?;
            let index: usize = token.get(open + 1..close)?.parse().ok()?;
            (!name.is_empty() && index >= 1).then(|| (name.to_string(), index))
        }
        None => (!token.is_empty()).then(|| (token.to_string(), 1)),
    }
}

/// `"text().OFFSET"` -> `OFFSET`; `"text()"` -> `0`. `None` for other tokens.
fn parse_text_offset(token: &str) -> Option<usize> {
    let rest = token.strip_prefix("text()")?;
    if rest.is_empty() {
        return Some(0);
    }
    rest.strip_prefix('.')?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_xpointer() {
        let xp = parse("/body/DocFragment[14]/body/div/p[3]/text().42").unwrap();
        assert_eq!(xp.doc_fragment, 14);
        assert_eq!(xp.steps, vec![("div".to_string(), 1), ("p".to_string(), 3)]);
        assert_eq!(xp.text_offset, 42);
    }

    #[test]
    fn parses_chapter_start() {
        let xp = parse("/body/DocFragment[1]/body/p[1]/text().0").unwrap();
        assert_eq!(xp.doc_fragment, 1);
        assert_eq!(xp.text_offset, 0);
    }

    #[test]
    fn rejects_bare_percentage() {
        assert!(parse("0.53000000").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn resolves_chapter_start_to_zero() {
        let html = "<html><body><h2>Title</h2><p>First para.</p><p>Second.</p></body></html>";
        // Points at the <h2>, the very first element -> offset 0.
        let xp = XPointer {
            doc_fragment: 1,
            steps: vec![("h2".to_string(), 1)],
            text_offset: 0,
        };
        assert_eq!(resolve_fraction(html, &xp), Some(0.0));
    }

    #[test]
    fn resolves_interior_paragraph() {
        // Body text: "Title"(5) + "First para."(11) + "Second."(7) = 23 chars.
        let html = "<html><body><h2>Title</h2><p>First para.</p><p>Second.</p></body></html>";
        // Target the second <p>; 16 chars precede it.
        let xp = XPointer {
            doc_fragment: 1,
            steps: vec![("p".to_string(), 2)],
            text_offset: 0,
        };
        let fraction = resolve_fraction(html, &xp).unwrap();
        assert!((fraction - 16.0 / 23.0).abs() < 1e-9);
    }

    #[test]
    fn missing_step_returns_none() {
        let html = "<html><body><p>Only one.</p></body></html>";
        let xp = XPointer {
            doc_fragment: 1,
            steps: vec![("p".to_string(), 5)],
            text_offset: 0,
        };
        assert_eq!(resolve_fraction(html, &xp), None);
    }
}
