#[cfg(test)]
mod tests {
    use repy::parser::parse_html;
    use std::collections::HashSet;

    #[test]
    fn test_footnote_jumping_reproduction() {
        let html = r##"
        <p><strong>[2]</strong> From what I’ve been told and remember of my natural father:<a epub:type="noteref" href="#fn67fn" id="fn67"><sup>3</sup></a> modesty and manliness.</p>

        <p epub:type="footnote" class="footnote" id="fn67fn"><a href="#fn67">3</a>. Also called Marcus Annius Verus; he died when Marcus was quite young. Marcus was then brought up by his grandfather, yet another Marcus Annius Verus.</p>
        "##;

        let mut section_ids = HashSet::new();
        section_ids.insert("fn67fn".to_string());
        section_ids.insert("fn67".to_string());

        let result = parse_html(html, Some(80), Some(section_ids), 0).unwrap();

        // Check if the section "fn67fn" (the footnote definition) was found and mapped to a valid line
        // The footnote definition text is "3. Also called Marcus Annius Verus..."
        // This should appear in the text lines.

        println!("Text lines:");
        for (i, line) in result.text_lines.iter().enumerate() {
            println!("{}: {}", i, line);
        }

        println!("Sections:");
        for (id, line) in &result.section_rows {
            println!("{}: {}", id, line);
        }

        // We expect "fn67fn" to be mapped to the line containing "3. Also called..."
        // If it's mapped to the end of the text (fallback), that's the bug (or at least partial failure).
        let footnote_line = result.section_rows.get("fn67fn");
        assert!(footnote_line.is_some(), "Footnote ID not found in sections");

        // Find the line index where the text actually is
        // Note: html2text might render "3." separately or differently.
        let actual_line_idx = result
            .text_lines
            .iter()
            .position(|l| l.contains("Also called Marcus"));
        assert!(
            actual_line_idx.is_some(),
            "Footnote text not found in output"
        );

        // The mapped line should be close to the actual line (within 1 line)
        if let Some(&mapped_line) = footnote_line {
            let diff = (mapped_line as isize - actual_line_idx.unwrap() as isize).abs();
            assert!(
                diff <= 1,
                "Footnote ID mapped to line {}, but text is at {}",
                mapped_line,
                actual_line_idx.unwrap()
            );
        }
    }

    #[test]
    fn test_duplicate_links_reproduction() {
        let html = r##"
        <p>Text<a epub:type="noteref" href="#fn67fn" id="fn67"><sup>3</sup></a></p>
        <p epub:type="footnote" class="footnote" id="fn67fn">
            <a href="#fn67">3</a>. Footnote text with <a href="http://google.com">External Link</a> and <a href="#chapter1">See Chapter 1</a>.
        </p>
        "##;

        let result = parse_html(html, Some(80), None, 0).unwrap();

        println!("Links:");
        for link in &result.links {
            println!("Label: {}, URL: {}", link.label, link.url);
        }

        // 1. Backlink "3" should be excluded
        let backlink_count = result.links.iter().filter(|l| l.url == "#fn67").count();
        assert_eq!(
            backlink_count, 0,
            "Backlink from footnote should be excluded"
        );

        // 2. External link should be PRESENT
        let external_count = result
            .links
            .iter()
            .filter(|l| l.url == "http://google.com")
            .count();
        assert_eq!(
            external_count, 1,
            "External link in footnote should be preserved"
        );

        // 3. Long internal link should be PRESENT (heuristic)
        let internal_count = result.links.iter().filter(|l| l.url == "#chapter1").count();
        assert_eq!(
            internal_count, 1,
            "Long internal link in footnote should be preserved"
        );
    }
}
