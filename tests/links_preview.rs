#[cfg(test)]
mod tests {
    use repy::parser::parse_html;
    use repy::ui::board::Board;
    use std::collections::HashSet;

    #[test]
    fn test_internal_link_resolution_in_parser() {
        let html = r##"
        <p>Go to <a href="#target">the target</a> section.</p>
        <div style="height: 1000px">Spacer</div>
        <h2 id="target">Target Section</h2>
        <p>This is the content of the target section.</p>
        "##;

        let mut section_ids = HashSet::new();
        section_ids.insert("target".to_string());

        let result = parse_html(html, Some(80), Some(section_ids), 0).unwrap();

        // Verify that the link was extracted
        let link = result.links.iter().find(|l| l.url == "#target").expect("Link to #target not found");
        assert_eq!(link.label, "the target");
        
        // Verify that the target section was mapped
        let target_row = *result.section_rows.get("target").expect("Section #target not found");
        assert!(target_row > 0);
        
        // Verify that the target content is at the mapped row
        assert!(result.text_lines[target_row].contains("Target Section"));
    }

    #[test]
    fn test_preview_content_extraction() {
        let text_lines = vec![
            "Line 0".to_string(),
            "Line 1".to_string(),
            "Target Line".to_string(),
            "Preview 1".to_string(),
            "Preview 2".to_string(),
        ];
        
        let board = Board::new().with_text_structure(repy::models::TextStructure {
            text_lines,
            ..Default::default()
        });

        // Test get_line which is used for preview
        assert_eq!(board.get_line(2), Some("Target Line"));
        assert_eq!(board.get_line(3), Some("Preview 1"));
        assert_eq!(board.get_line(5), None);
    }
}
