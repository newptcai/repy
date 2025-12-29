use repy::parser::parse_html;

#[test]
fn test_wrapped_formatting_markers() {
    // A string long enough to wrap at 80 chars
    let long_bold_text = "This is a very long bold text segment that is definitely going to wrap because it is way longer than eighty characters and html2text will have to split it somewhere.";
    let html = format!("<p>Start <strong>{}</strong> End</p>", long_bold_text);

    // Parse with width 80
    let result = parse_html(&html, Some(80), None, 0).unwrap();

    // Verify we have multiple lines
    assert!(result.text_lines.len() > 1, "Text did not wrap as expected");

    // Check that markers are stripped
    // If bug exists, we might see "**This" or "somewhere.**"
    for line in &result.text_lines {
        println!("Line: {}", line);
        assert!(!line.contains("**This"), "Start marker not stripped: {}", line);
        assert!(!line.contains("somewhere.**"), "End marker not stripped: {}", line);
        // Also check general markers just in case
        assert!(!line.contains("**"), "Bold markers remaining in line: {}", line);
    }

    // Verify formatting is detected
    let bold_styles: Vec<_> = result.formatting.iter().filter(|s| s.attr == 1).collect();
    assert!(!bold_styles.is_empty(), "No bold formatting detected");
    
    // We expect at least two segments of bold text (since it wrapped)
    assert!(bold_styles.len() >= 2, "Expected bold formatting to be split across lines, found {} segments", bold_styles.len());
}
