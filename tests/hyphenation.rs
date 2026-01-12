use repy::parser::parse_html;
use std::collections::HashSet;

#[test]
fn test_hyphenation_and_wrapping() {
    let html = r#"
    <p>This is a paragraph with a very long word: pneumonoultramicroscopicsilicovolcanoconiosis. It should be hyphenated.</p>
    <ul>
        <li>Item 1 is long enough to wrap to the next line and should be indented.</li>
        <li>Item 2</li>
    </ul>
    "#;

    let width = 30;
    let result = parse_html(html, Some(width), None, 0).unwrap();

    for line in &result.text_lines {
        println!("|{}|", line);
        assert!(line.len() <= width, "Line exceeds width: '{}'", line);
    }

    // Check for hyphenation
    let joined = result.text_lines.join("\n");
    assert!(joined.contains("-"), "Should contain hyphenation");
    
    // Check for indentation
    // Find the line starting with "* Item 1"
    let item_start_idx = result.text_lines.iter().position(|l| l.starts_with("* Item 1")).unwrap();
    // The next line should be indented
    let next_line = &result.text_lines[item_start_idx + 1];
    assert!(next_line.starts_with("  "), "List item should be indented: '{}'", next_line);
}
