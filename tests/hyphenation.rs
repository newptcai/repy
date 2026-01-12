use repy::parser::parse_html;
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

#[test]
fn test_footnote_wrapping_expected_lines() {
    let html = r#"
    <p>[3] From my mother:<sup>4</sup> reverence for the gods, generosity, and the ability to abstain not only from wrongdoing
    but even from contemplating it; also, a frugal lifestyle, far removed from the habits of the rich.<sup>5</sup></p>
    "#;

    let cases: Vec<(usize, Vec<&str>)> = vec![
        (70, vec![
            "[3] From my mother:^{4} reverence for the gods, generosity, and the",
            "ability to abstain not only from wrongdoing but even from contem-",
            "plating it; also, a frugal lifestyle, far removed from the habits of",
            "the rich.^{5}",
        ]),
        (80, vec![
            "[3] From my mother:^{4} reverence for the gods, generosity, and the ability to",
            "abstain not only from wrongdoing but even from contemplating it; also, a frugal",
            "lifestyle, far removed from the habits of the rich.^{5}",
        ]),
        (100, vec![
            "[3] From my mother:^{4} reverence for the gods, generosity, and the ability to abstain not only from",
            "wrongdoing but even from contemplating it; also, a frugal lifestyle, far removed from the habits of",
            "the rich.^{5}",
        ]),
    ];

    for (width, expected) in cases {
        let result = parse_html(html, Some(width), None, 0).unwrap();
        let expected_lines: Vec<String> = expected.into_iter().map(|line| line.to_string()).collect();
        assert_eq!(result.text_lines, expected_lines, "Unexpected wrapping at width {}", width);
    }
}
