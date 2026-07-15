//! Rendering layer: turns raw [`ChapterContent`] from a format backend into
//! wrapped, styled [`TextStructure`]s via the shared HTML parse pipeline.
//! Non-HTML payloads are converted to minimal HTML first so every format
//! flows through the same wrapping, styling, and image machinery.

use crate::formats::{ChapterContent, Ebook, resolve_relative_resource};
use crate::models::{CHAPTER_BREAK_MARKER, TextStructure};
use crate::parser::{InlineImageOptions, parse_html_with_styles};
use eyre::Result;
use std::collections::{HashMap, HashSet};

/// Parse a single chapter into a wrapped [`TextStructure`].
pub fn parse_chapter(
    ebook: &mut dyn Ebook,
    index: usize,
    text_width: usize,
    starting_line: usize,
    inline_image_rows: Option<usize>,
) -> Result<TextStructure> {
    let html = chapter_html(ebook.get_chapter(index)?);

    // Collect section IDs from the table of contents
    let section_ids: HashSet<String> = ebook
        .toc_entries()
        .iter()
        .filter_map(|entry| entry.section.clone())
        .collect();

    let inline_options = inline_image_rows.map(|max_rows| InlineImageOptions {
        dimensions: collect_image_dimensions(ebook, &html, index),
        max_rows,
    });

    parse_html_with_styles(
        &html,
        Some(text_width),
        Some(section_ids),
        starting_line,
        ebook.styled_classes(),
        inline_options.as_ref(),
    )
}

/// Parse every chapter of the book, keeping global line numbers continuous
/// and (optionally) padding each chapter to a page boundary with a break
/// marker so chapters start on a fresh page.
pub fn parse_book(
    ebook: &mut dyn Ebook,
    text_width: usize,
    page_height: Option<usize>,
    inline_image_rows: Option<usize>,
) -> Result<Vec<TextStructure>> {
    let mut all_content = Vec::new();
    let mut starting_line = 0;
    let total_chapters = ebook.contents().len();

    for index in 0..total_chapters {
        let mut parsed_content =
            parse_chapter(ebook, index, text_width, starting_line, inline_image_rows)?;
        if let Some(page_height) = page_height
            && index + 1 < total_chapters
        {
            let total_lines = starting_line + parsed_content.text_lines.len();
            let break_lines = build_chapter_break(page_height, total_lines);
            parsed_content.text_lines.extend(break_lines);
        }
        starting_line += parsed_content.text_lines.len();
        all_content.push(parsed_content);
    }

    Ok(all_content)
}

/// Convert a chapter payload to the HTML the parse pipeline consumes.
fn chapter_html(content: ChapterContent) -> String {
    match content {
        ChapterContent::Html(html) => html,
        ChapterContent::PlainText(text) => plain_text_to_html(&text),
        ChapterContent::Markdown(text) => markdown_to_html(&text),
        // The leading slash makes the src book-root-relative, so resolving
        // it against any chapter base path yields the archive entry name.
        ChapterContent::ImagePage(path) => {
            format!("<img src=\"/{}\"/>", escape_html(&path))
        }
    }
}

/// Blank-line-separated paragraphs become `<p>` elements; hard-wrapped lines
/// inside a paragraph reflow, since HTML collapses the newlines.
fn plain_text_to_html(text: &str) -> String {
    let text = text.replace("\r\n", "\n");
    let mut html = String::with_capacity(text.len() + 64);
    for paragraph in text.split("\n\n") {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        html.push_str("<p>");
        html.push_str(&escape_html(paragraph));
        html.push_str("</p>\n");
    }
    html
}

fn markdown_to_html(text: &str) -> String {
    use pulldown_cmark::{Options, Parser, html};

    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS;
    let parser = Parser::new_ext(text, options);
    let mut html_out = String::with_capacity(text.len() * 2);
    html::push_html(&mut html_out, parser);
    html_out
}

fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Pixel dimensions (header-only decode) for every `<img src>` in a chapter,
/// keyed by the raw src attribute value. Images that cannot be resolved or
/// decoded (e.g. SVG) are simply absent. SVG-wrapped raster images are
/// normalized first, matching the parser's preprocessing, so the keys line
/// up with the srcs the parser extracts.
fn collect_image_dimensions(
    ebook: &mut dyn Ebook,
    raw_html: &str,
    index: usize,
) -> HashMap<String, (u32, u32)> {
    let raw_html = crate::parser::preprocess_svg_images(raw_html);
    let fragment = scraper::Html::parse_fragment(&raw_html);
    let selector = scraper::Selector::parse("img").unwrap();
    let sources: Vec<String> = fragment
        .select(&selector)
        .filter_map(|el| el.value().attr("src").map(str::to_string))
        .collect();
    drop(fragment);

    // Relative srcs resolve against the chapter document's path.
    let base_path = ebook.spine_href(index);

    let mut dimensions = HashMap::new();
    for src in sources {
        if dimensions.contains_key(&src) {
            continue;
        }
        let resolved =
            resolve_relative_resource(&src, base_path.as_deref()).unwrap_or_else(|| src.clone());
        let Ok((_mime, bytes)) = ebook.get_resource(&resolved) else {
            continue;
        };
        if let Ok(reader) =
            image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()
            && let Ok(dims) = reader.into_dimensions()
        {
            dimensions.insert(src, dims);
        }
    }
    dimensions
}

pub fn build_chapter_break(page_height: usize, total_lines: usize) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(String::new());
    lines.push(CHAPTER_BREAK_MARKER.to_string());
    if page_height == 0 {
        return lines;
    }
    let remainder = (total_lines + lines.len()) % page_height;
    let pad = if remainder == 0 {
        0
    } else {
        page_height - remainder
    };
    lines.extend(std::iter::repeat_n(String::new(), pad));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::Epub;

    fn small_epub() -> Result<Epub> {
        let mut epub = Epub::new("tests/fixtures/small.epub");
        epub.initialize()?;
        Ok(epub)
    }

    #[test]
    fn test_parse_chapter_small() -> Result<()> {
        let mut epub = small_epub()?;
        let parsed_content = parse_chapter(&mut epub, 0, 80, 0, None)?;

        assert!(!parsed_content.text_lines.is_empty());

        // Check that lines are reasonable length (due to wrapping)
        for line in &parsed_content.text_lines {
            assert!(line.len() <= 80 || !line.contains(' ')); // Either wrapped or single long word
        }

        Ok(())
    }

    #[test]
    fn test_inline_image_rows() -> Result<()> {
        let mut epub = small_epub()?;

        // The first chapter is the cover page with one image.
        let plain = parse_chapter(&mut epub, 0, 80, 0, None)?;
        let inline = parse_chapter(&mut epub, 0, 80, 0, Some(20))?;

        assert!(plain.image_block_rows.is_empty());
        let (&row, &rows) = inline
            .image_block_rows
            .iter()
            .next()
            .expect("cover image should reserve a block");
        assert!((2..=20).contains(&rows));
        assert!(inline.image_maps.contains_key(&row));
        assert_eq!(
            inline.text_lines.len(),
            plain.text_lines.len() + rows - 1,
            "reserved rows extend the chapter"
        );
        Ok(())
    }

    #[test]
    fn test_parse_chapter_with_wrapping() -> Result<()> {
        let mut epub = small_epub()?;

        let narrow_content = parse_chapter(&mut epub, 0, 40, 0, None)?;
        let wide_content = parse_chapter(&mut epub, 0, 120, 0, None)?;

        // Both should parse successfully without crashing
        assert!(!narrow_content.text_lines.is_empty() || !wide_content.text_lines.is_empty());

        Ok(())
    }

    #[test]
    fn test_parse_chapter_with_line_offset() -> Result<()> {
        let mut epub = small_epub()?;

        let starting_line = 1000;
        let parsed_content = parse_chapter(&mut epub, 0, 80, starting_line, None)?;

        assert!(!parsed_content.text_lines.is_empty());

        // Check that images and sections are offset properly
        for &line_num in parsed_content.image_maps.keys() {
            assert!(line_num >= starting_line);
        }

        for &line_num in parsed_content.section_rows.values() {
            assert!(line_num >= starting_line);
        }

        for style in &parsed_content.formatting {
            assert!(style.row >= starting_line as u16);
        }

        Ok(())
    }

    #[test]
    fn test_parse_book_small() -> Result<()> {
        let mut epub = small_epub()?;

        let all_content = parse_book(&mut epub, 80, None, None)?;

        // Should have content for all chapters
        assert_eq!(all_content.len(), epub.contents().len());

        // Each content should have text lines
        for content in &all_content {
            assert!(!content.text_lines.is_empty());
        }

        // Total lines should be substantial
        let total_lines: usize = all_content.iter().map(|c| c.text_lines.len()).sum();
        assert!(total_lines > 1000); // Should be a large book

        Ok(())
    }

    #[test]
    fn test_parse_book_line_continuity() -> Result<()> {
        let mut epub = small_epub()?;

        let all_content = parse_book(&mut epub, 80, None, None)?;
        let mut current_line = 0;

        // Check that line numbers are continuous across chapters
        for text_structure in &all_content {
            for &line_num in text_structure.section_rows.values() {
                assert!(line_num >= current_line);
            }

            for &line_num in text_structure.image_maps.keys() {
                assert!(line_num >= current_line);
            }

            for style in &text_structure.formatting {
                assert!(style.row >= current_line as u16);
            }

            current_line += text_structure.text_lines.len();
        }

        Ok(())
    }

    #[test]
    fn test_parse_book_performance() -> Result<()> {
        let mut epub = Epub::new("tests/fixtures/meditations.epub");
        epub.initialize()?;

        let start = std::time::Instant::now();
        let all_content = parse_book(&mut epub, 80, None, None)?;
        let duration = start.elapsed();
        assert!(duration.as_secs() < 10); // Should complete in under 10 seconds

        assert_eq!(all_content.len(), epub.contents().len());

        Ok(())
    }

    #[test]
    fn test_plain_text_to_html_paragraphs() {
        let html = plain_text_to_html("First para,\nstill first.\n\nSecond & <last>.");
        assert_eq!(
            html,
            "<p>First para,\nstill first.</p>\n<p>Second &amp; &lt;last&gt;.</p>\n"
        );
    }

    #[test]
    fn test_plain_text_to_html_crlf_and_blanks() {
        let html = plain_text_to_html("a\r\n\r\nb\n\n\n\nc");
        assert_eq!(html, "<p>a</p>\n<p>b</p>\n<p>c</p>\n");
    }

    #[test]
    fn test_markdown_to_html_basics() {
        let html = markdown_to_html("# Title\n\nSome *emphasis* and **bold**.\n\n- a\n- b");
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<em>emphasis</em>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<li>a</li>"));
    }

    #[test]
    fn test_markdown_chapter_parses_with_formatting() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("book.md");
        std::fs::write(&path, "# A Heading\n\nPlain *italic words here* end.")?;

        let mut book = crate::formats::open(&path.to_string_lossy())?;
        let parsed = parse_chapter(book.as_mut(), 0, 80, 0, None)?;
        let text = parsed.text_lines.join("\n");
        assert!(text.contains("A Heading"));
        assert!(text.contains("italic words here"));
        assert!(
            !parsed.formatting.is_empty(),
            "emphasis should survive the pipeline as formatting"
        );
        Ok(())
    }

    #[test]
    fn test_chapter_html_image_page() {
        let html = chapter_html(ChapterContent::ImagePage("pages/001.png".to_string()));
        assert_eq!(html, "<img src=\"/pages/001.png\"/>");
        // Book-root-relative srcs resolve to the archive entry from any base.
        assert_eq!(
            crate::formats::resolve_relative_resource("/pages/001.png", Some("pages/001.png")),
            Some("pages/001.png".to_string())
        );
    }

    #[test]
    fn test_plain_text_chapter_parses() -> Result<()> {
        struct TextBook {
            contents: Vec<String>,
            toc: Vec<crate::models::TocEntry>,
            meta: crate::models::BookMetadata,
        }
        impl Ebook for TextBook {
            fn path(&self) -> &str {
                "book.txt"
            }
            fn contents(&self) -> &Vec<String> {
                &self.contents
            }
            fn toc_entries(&self) -> &Vec<crate::models::TocEntry> {
                &self.toc
            }
            fn get_meta(&self) -> &crate::models::BookMetadata {
                &self.meta
            }
            fn spine_href(&self, _index: usize) -> Option<String> {
                Some("book.txt".to_string())
            }
            fn initialize(&mut self) -> Result<()> {
                Ok(())
            }
            fn get_chapter(&mut self, _index: usize) -> Result<ChapterContent> {
                Ok(ChapterContent::PlainText(
                    "One two three.\n\nFour five.".to_string(),
                ))
            }
            fn get_resource(&mut self, _path: &str) -> Result<(String, Vec<u8>)> {
                Err(eyre::eyre!("no resources"))
            }
            fn cleanup(&mut self) -> Result<()> {
                Ok(())
            }
        }

        let mut book = TextBook {
            contents: vec!["book.txt".to_string()],
            toc: Vec::new(),
            meta: crate::models::BookMetadata::default(),
        };
        let parsed = parse_chapter(&mut book, 0, 80, 0, None)?;
        let text = parsed.text_lines.join("\n");
        assert!(text.contains("One two three."));
        assert!(text.contains("Four five."));
        Ok(())
    }

    #[test]
    fn test_build_chapter_break_pads_to_page() {
        let lines = build_chapter_break(10, 13);
        // 2 marker lines + padding to the next multiple of 10
        assert_eq!(lines.len(), 2 + 5);
        assert_eq!(lines[1], CHAPTER_BREAK_MARKER);
    }

    #[test]
    fn test_build_chapter_break_zero_height() {
        assert_eq!(build_chapter_break(0, 42).len(), 2);
    }
}
