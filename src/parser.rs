use crate::models::{TextStructure, InlineStyle};
use eyre::Result;
use html2text::from_read;
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

    // Convert HTML to plain text first
    let plain_text = html_to_plain_text(html_src, text_width)?;

    // Extract structure information
    let image_maps = extract_images(html_src, starting_line)?;
    let section_rows = extract_sections(html_src, &section_ids.unwrap_or_default(), starting_line, &plain_text)?;
    let formatting = extract_formatting(html_src, starting_line, &plain_text)?;

    Ok(TextStructure {
        text_lines: plain_text,
        image_maps,
        section_rows,
        formatting,
    })
}

/// Convert HTML to plain text using html2text library
fn html_to_plain_text(html: &str, width: usize) -> Result<Vec<String>> {
    let text = from_read(html.as_bytes(), width)?;
    let lines: Vec<String> = text.lines().map(|s| s.to_string()).collect();
    Ok(lines)
}

/// Extract image information from HTML
fn extract_images(html: &str, starting_line: usize) -> Result<HashMap<usize, String>> {
    let mut images = HashMap::new();
    let fragment = Html::parse_fragment(html);

    let img_selector = Selector::parse("img").unwrap();

    for (line_num, element) in fragment.select(&img_selector).enumerate() {
        if let Some(src) = element.value().attr("src") {
            images.insert(starting_line + line_num, src.to_string());
        }
    }

    Ok(images)
}

/// Extract section information from HTML
fn extract_sections(
    html: &str,
    section_ids: &HashSet<String>,
    starting_line: usize,
    text_lines: &[String],
) -> Result<HashMap<String, usize>> {
    let mut sections = HashMap::new();

    if section_ids.is_empty() {
        return Ok(sections);
    }

    let fragment = Html::parse_fragment(html);

    // Look for elements with id attributes that match our section IDs
    let id_selector = Selector::parse("*[id]").unwrap();

    for element in fragment.select(&id_selector) {
        if let Some(id) = element.value().attr("id") {
            if section_ids.contains(id) {
                // Estimate the line number where this section starts
                // This is approximate since html2text changes the structure
                let element_text = element.text().collect::<String>();

                // Find the closest line in our output that contains this element's text
                for (line_num, line) in text_lines.iter().enumerate() {
                    if line.contains(&element_text) || element_text.contains(line) {
                        sections.insert(id.to_string(), starting_line + line_num);
                        break;
                    }
                }

                // If we didn't find an exact match, use the current line count as a fallback
                if !sections.contains_key(id) {
                    sections.insert(id.to_string(), starting_line + text_lines.len());
                }
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
        assert_eq!(result.text_lines[2], "This is a **bold** paragraph with some *italic* text.");

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
    fn test_extract_images() {
        let html = r#"<p>Here's an image: <img src="test.jpg" alt="Test Image"></p>"#;
        let images = extract_images(html, 0).unwrap();
        assert_eq!(images.len(), 1);
        assert_eq!(images.get(&0), Some(&"test.jpg".to_string()));
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
    fn test_extract_formatting() {
        let html = "<p>This is <strong>bold</strong> and <em>italic</em>.</p>";
        let text_lines = vec!["This is **bold** and *italic*.".to_string()];
        let formatting = extract_formatting(html, 0, &text_lines).unwrap();
        assert_eq!(formatting.len(), 2);
        assert!(formatting.iter().any(|s| s.n_letters == 4 && s.attr == 1)); // bold
        assert!(formatting.iter().any(|s| s.n_letters == 6 && s.attr == 2)); // italic
    }
}