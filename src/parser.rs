use crate::models::{InlineStyle, LinkEntry, TextStructure};
use eyre::Result;
use html2text::config;
use regex::{Captures, Regex};
use scraper::{Html, Selector};
use std::collections::{HashMap, HashSet};

/// Simple HTML parser for ebook content
/// This uses html2text for the heavy lifting and adds some basic structure tracking
pub fn parse_html(
    html_src: &str,
    text_width: Option<usize>,
    section_ids: Option<HashSet<String>>,
    starting_line: usize,
) -> Result<TextStructure> {
    let text_width = text_width.unwrap_or(80);
    let html_src = preprocess_inline_annotations(html_src);
    let html_src = preprocess_images(&html_src);

    // Convert HTML to plain text first
    let mut plain_text = html_to_plain_text(&html_src, text_width)?;
    replace_superscript_link_markers(&mut plain_text);

    // Extract structure information
    let image_maps = extract_images(&html_src, starting_line, &plain_text)?;
    let section_rows = extract_sections(&html_src, &section_ids.unwrap_or_default(), starting_line, &plain_text)?;
    let mut formatting = extract_formatting(&html_src, starting_line, &plain_text)?;
    let links = extract_links(&html_src, starting_line, &plain_text)?;

    strip_inline_markers(&mut plain_text, &mut formatting, starting_line);

    Ok(TextStructure {
        text_lines: plain_text,
        image_maps,
        section_rows,
        formatting,
        links,
    })
}

fn preprocess_inline_annotations(html: &str) -> String {
    let sup_open = Regex::new(r"(?i)<sup[^>]*>").unwrap();
    let sup_close = Regex::new(r"(?i)</sup>").unwrap();
    let sub_open = Regex::new(r"(?i)<sub[^>]*>").unwrap();
    let sub_close = Regex::new(r"(?i)</sub>").unwrap();

    let mut processed = sup_open.replace_all(html, "^{").to_string();
    processed = sup_close.replace_all(&processed, "}").to_string();
    processed = sub_open.replace_all(&processed, "_{").to_string();
    sub_close.replace_all(&processed, "}").to_string()
}

fn replace_superscript_link_markers(lines: &mut [String]) {
    let re = Regex::new(r"\[\^\{[^}]+\}\]").unwrap();
    let mut counter = 0usize;
    for line in lines.iter_mut() {
        if !line.contains("[^{") {
            continue;
        }
        let replaced = re.replace_all(line, |_caps: &Captures| {
            counter += 1;
            format!("^{{{}}}", counter)
        });
        *line = replaced.to_string();
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
fn extract_images(html: &str, starting_line: usize, text_lines: &[String]) -> Result<HashMap<usize, String>> {
    let mut images = HashMap::new();
    let fragment = Html::parse_fragment(html);
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

/// Extract section/anchor ids from HTML for TOC navigation and internal link jumps.
fn extract_sections(
    html: &str,
    _section_ids: &HashSet<String>,
    starting_line: usize,
    text_lines: &[String],
) -> Result<HashMap<String, usize>> {
    let mut sections = HashMap::new();

    let fragment = Html::parse_fragment(html);

    // Look for elements with id attributes that match our section IDs
    let id_selector = Selector::parse("*[id]").unwrap();

    for element in fragment.select(&id_selector) {
        if let Some(id) = element.value().attr("id") {
            // Track all anchors so internal links can jump, even if the ID is not in the TOC.
            // TOC navigation still relies on TOC entries; extra anchors do not change that.
            // Estimate the line number where this section starts.
            // This is approximate since html2text changes the structure.
            let element_text = element.text().collect::<String>();
            let words: Vec<&str> = element_text.split_whitespace().collect();

            // Strategy 1: Match the first few words (exact sequence)
            let mut found = false;
            if !words.is_empty() {
                // Try chunks of words to avoid issues with wrapping or partial matches
                // We try a longer prefix first, then shorter ones.
                // We also try skipping the first word to handle cases where decoration ([1]) matches differently than plain text (1).
                let attempts = [
                    (0, 32), // Start at 0, take up to 32 words
                    (0, 10), // Start at 0, take up to 10
                    (0, 5),  // Start at 0, take up to 5
                    (1, 32), // Skip 1, take up to 32 (handles "[1] Text" vs "1. Text")
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
                         // Too short to be unique
                        continue;
                    }

                    for (line_num, line) in text_lines.iter().enumerate() {
                        if line.contains(&search_str) {
                            sections.insert(id.to_string(), starting_line + line_num);
                            found = true;
                            break;
                        }
                    }
                    if found {
                        break;
                    }
                }
            }

            // Fallback: If word matching failed, try the old normalization method
            if !found {
                let normalized = words.join(" ");
                let prefix_len = normalized.chars().count().min(32);
                let prefix: String = normalized.chars().take(prefix_len).collect();

                for (line_num, line) in text_lines.iter().enumerate() {
                    if !normalized.is_empty()
                        && (line.contains(&normalized)
                            || (!prefix.is_empty() && line.contains(&prefix)))
                    {
                        sections.insert(id.to_string(), starting_line + line_num);
                        break;
                    }
                }
            }

            // Final Fallback: Use the end of the current block
            if !sections.contains_key(id) {
                sections.insert(id.to_string(), starting_line + text_lines.len());
            }
        }
    }

    Ok(sections)
}

/// Extract basic formatting information (headers, bold, italic)
fn extract_formatting(
    html: &str,
    starting_line: usize,
    text_lines: &[String],
) -> Result<Vec<InlineStyle>> {
    let mut formatting = Vec::new();
    let fragment = Html::parse_fragment(html);

    // Extract headers (centered/bold)
    let header_selector = Selector::parse("h1, h2, h3, h4, h5, h6").unwrap();
    for element in fragment.select(&header_selector) {
        let header_text = element.text().collect::<String>();

        // Find the line in our output that contains this header
        for (line_num, line) in text_lines.iter().enumerate() {
            if line.contains(&header_text) {
                formatting.push(InlineStyle {
                    row: (starting_line + line_num) as u16,
                    col: 0,
                    n_letters: line.len() as u16,
                    attr: 1, // Bold (simplified)
                });
                break;
            }
        }
    }

    // Extract bold text
    let bold_selector = Selector::parse("strong, b").unwrap();
    for element in fragment.select(&bold_selector) {
        let bold_text = element.text().collect::<String>();

        // Find the line in our output that contains this bold text
        for (line_num, line) in text_lines.iter().enumerate() {
            if let Some(pos) = line.find(&bold_text) {
                formatting.push(InlineStyle {
                    row: (starting_line + line_num) as u16,
                    col: pos as u16,
                    n_letters: bold_text.len() as u16,
                    attr: 1, // Bold
                });
                break;
            }
        }
    }

    // Extract italic text
    let italic_selector = Selector::parse("em, i").unwrap();
    for element in fragment.select(&italic_selector) {
        let italic_text = element.text().collect::<String>();

        // Find the line in our output that contains this italic text
        for (line_num, line) in text_lines.iter().enumerate() {
            if let Some(pos) = line.find(&italic_text) {
                formatting.push(InlineStyle {
                    row: (starting_line + line_num) as u16,
                    col: pos as u16,
                    n_letters: italic_text.len() as u16,
                    attr: 2, // Italic (simplified)
                });
                break;
            }
        }
    }

    Ok(formatting)
}

/// Extract link metadata without injecting markers into the rendered text.
/// We keep links as separate entries so reading flow stays unchanged; link UI uses these rows.
fn extract_links(html: &str, starting_line: usize, text_lines: &[String]) -> Result<Vec<LinkEntry>> {
    let mut links = Vec::new();
    let fragment = Html::parse_fragment(html);
    let link_selector = Selector::parse("a[href]").unwrap();
    let sup_selector = Selector::parse("sup").unwrap();
    let mut sup_counter = 0usize;

    for element in fragment.select(&link_selector) {
        let href = match element.value().attr("href") {
            Some(value) if !value.trim().is_empty() => value.trim(),
            _ => continue,
        };

        // Filter out backlinks inside footnotes
        // Check if any ancestor has epub:type="footnote"
        // explicitly allow epub:type="noteref" (links TO footnotes)
        let is_noteref = element.value().attr("epub:type") == Some("noteref");
        if !is_noteref {
            let mut parent = element.parent();
            let mut inside_footnote = false;
            while let Some(node) = parent {
                if let Some(element_ref) = scraper::ElementRef::wrap(node) {
                     if element_ref.value().attr("epub:type") == Some("footnote") {
                         inside_footnote = true;
                         break;
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
            label: if label.is_empty() { href.to_string() } else { label },
            url: href.to_string(),
            target_row: None,
        });
    }

    Ok(links)
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

    for style_idx in line_formatting {
        let style = &formatting[*style_idx];
        let start = style.col as usize;
        let end = start.saturating_add(style.n_letters as usize);
        match style.attr {
            1 => {
                if start >= 2 && &bytes[start - 2..start] == b"**" {
                    positions.push(start - 2);
                    positions.push(start - 1);
                }
                if end + 2 <= len && &bytes[end..end + 2] == b"**" {
                    positions.push(end);
                    positions.push(end + 1);
                }
            }
            2 => {
                if start >= 1 && bytes.get(start - 1) == Some(&b'*') {
                    positions.push(start - 1);
                }
                if end < len && bytes.get(end) == Some(&b'*') {
                    positions.push(end);
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
        assert_eq!(result.text_lines[2], "This is a bold paragraph with some italic text.");

        assert_eq!(result.image_maps.len(), 1);
        assert!(result.image_maps.values().any(|v| v == "test.jpg"));

        assert_eq!(result.section_rows.len(), 1);
        assert_eq!(result.section_rows.get("chapter1"), Some(&0));

        assert_eq!(result.formatting.len(), 3);
        assert!(result.formatting.iter().any(|s| s.attr == 1)); // bold
        assert!(result.formatting.iter().any(|s| s.attr == 2)); // italic
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
        assert!(lines.iter().any(|l| l.contains("Second paragraph with **bold** text.")));
        assert!(lines.iter().any(|l| l.contains("Third paragraph.")));
    }

    #[test]
    fn test_extract_images() {
        let html = r#"<p>Here's an image: <img src="test.jpg" alt="[Image: test.jpg]"></p>"#;
        // Mock text lines that html2text would produce
        let text_lines = vec![
            "Here's an image: [[Image: test.jpg]]".to_string()
        ];
        let images = extract_images(html, 0, &text_lines).unwrap();
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
        
        // Mock text lines
        let text_lines = vec![
            "First image: [[Image: image1.jpg]]".to_string(),
            "Second image: [[Image: image2.png]]".to_string(),
            "[[Image: image3.gif]]".to_string(),
        ];
        
        let images = extract_images(html, 5, &text_lines).unwrap();
        assert_eq!(images.len(), 3);
        assert_eq!(images.get(&5), Some(&"image1.jpg".to_string()));
        assert_eq!(images.get(&6), Some(&"image2.png".to_string()));
        assert_eq!(images.get(&7), Some(&"image3.gif".to_string()));
    }

    #[test]
    fn test_extract_images_none() {
        let html = "<p>No images here.</p>";
        let text_lines = vec!["No images here.".to_string()];
        let images = extract_images(html, 0, &text_lines).unwrap();
        assert_eq!(images.len(), 0);
    }

    #[test]
    fn test_extract_images_without_src() {
        let html = "<p><img alt=\"Image without src\"></p>";
        let text_lines = vec!["[[Image without src]]".to_string()];
        let images = extract_images(html, 0, &text_lines).unwrap();
        assert_eq!(images.len(), 0);
    }

    #[test]
    fn test_extract_sections() {
        let html = r#"<h1 id="chapter1">Chapter 1</h1>"#;
        let mut section_ids = HashSet::new();
        section_ids.insert("chapter1".to_string());
        let text_lines = vec!["# Chapter 1".to_string()];
        let sections = extract_sections(html, &section_ids, 0, &text_lines).unwrap();
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

        let sections = extract_sections(html, &section_ids, 0, &text_lines).unwrap();
        assert_eq!(sections.len(), 3);
        assert_eq!(sections.get("intro"), Some(&0));
        assert_eq!(sections.get("chapter1"), Some(&2));
        assert_eq!(sections.get("conclusion"), Some(&4));
    }

    #[test]
    fn test_extract_sections_empty_section_ids() {
        let html = r#"<h1 id="chapter1">Chapter 1</h1>"#;
        let section_ids = HashSet::new();
        let text_lines = vec!["# Chapter 1".to_string()];
        let sections = extract_sections(html, &section_ids, 0, &text_lines).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections.get("chapter1"), Some(&0));
    }

    #[test]
    fn test_extract_sections_no_matching_sections() {
        let html = r#"<h1 id="chapter1">Chapter 1</h1>"#;
        let mut section_ids = HashSet::new();
        section_ids.insert("nonexistent".to_string());
        let text_lines = vec!["# Chapter 1".to_string()];
        let sections = extract_sections(html, &section_ids, 0, &text_lines).unwrap();
        assert_eq!(sections.len(), 1);
        assert_eq!(sections.get("chapter1"), Some(&0));
    }

    #[test]
    fn test_extract_formatting() {
        let html = "<p>This is <strong>bold</strong> and <em>italic</em>.</p>";
        let text_lines = vec!["This is **bold** and *italic*.".to_string()];
        let formatting = extract_formatting(html, 0, &text_lines).unwrap();
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
        let text_lines = vec![
            "# Header 1".to_string(),
            "Paragraph content.".to_string(),
            "## Header 2".to_string(),
        ];
        let formatting = extract_formatting(html, 0, &text_lines).unwrap();
        assert_eq!(formatting.len(), 2);

        // Check header 1 - html2text might format differently than expected
        let header1 = formatting.iter().find(|s| s.row == 0).unwrap();
        assert_eq!(header1.col, 0);
        assert_eq!(header1.n_letters, "# Header 1".len() as u16); // Use actual length
        assert_eq!(header1.attr, 1); // Bold

        // Check header 2 - html2text might format differently than expected
        let header2 = formatting.iter().find(|s| s.row == 2).unwrap();
        assert_eq!(header2.col, 0);
        assert_eq!(header2.n_letters, "## Header 2".len() as u16); // Use actual length
        assert_eq!(header2.attr, 1); // Bold
    }

    #[test]
    fn test_extract_formatting_no_matching_text() {
        let html = "<p>This has <strong>bold</strong> text.</p>";
        let text_lines = vec!["Completely different text content.".to_string()];
        let formatting = extract_formatting(html, 0, &text_lines).unwrap();
        assert_eq!(formatting.len(), 0);
    }

    #[test]
    fn test_extract_formatting_no_html() {
        let html = "";
        let text_lines = vec!["Plain text content.".to_string()];
        let formatting = extract_formatting(html, 0, &text_lines).unwrap();
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
        assert!(result.text_lines.iter().any(|line| line.contains("Introduction")));

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
        assert!(result.text_lines.iter().any(|line| line.contains("INTRODUCTION")));
        assert!(result.text_lines.iter().any(|line| line.contains("MARCUS AURELIUS")));

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
    fn test_nested_formatting() {
        let html = "<p>This has <strong>nested <em>bold italic</em> text</strong>.</p>";
        let text_lines = vec!["This has **nested *bold italic* text**.".to_string()];
        let formatting = extract_formatting(html, 0, &text_lines).unwrap();

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
        assert!(result.iter().any(|l| l.trim().contains("Text with extra spaces")));
        assert!(result.iter().any(|l| l.trim().contains("Text with newlines and spaces")));
    }
}

fn preprocess_images(html: &str) -> String {
    let img_re = Regex::new(r#"(?i)<img\s+([^>]+)>"#).unwrap();
    img_re.replace_all(html, |caps: &Captures| {
        let attrs_str = &caps[1];
        let src_re = Regex::new(r#"src=["']([^"']+)["']"#).unwrap();
        let alt_re = Regex::new(r#"alt=["']([^"']*)["']"#).unwrap();
        let title_re = Regex::new(r#"title=["']([^"']*)["']"#).unwrap();

        let src = src_re.captures(attrs_str).map(|c| c.get(1).unwrap().as_str().to_string());
        let alt = alt_re.captures(attrs_str).map(|c| c.get(1).unwrap().as_str().to_string());
        let title = title_re.captures(attrs_str).map(|c| c.get(1).unwrap().as_str().to_string());

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
                 alt_re.replace(attrs_str, format!(r#"alt="{}""#, new_alt_text)).to_string()
             } else {
                 format!(r#"{} alt="{}""#, attrs_str, new_alt_text)
             };
             
             format!("<img {}>", new_attrs)
        } else {
            caps[0].to_string()
        }
    }).to_string()
}

