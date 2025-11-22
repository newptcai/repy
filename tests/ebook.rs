use eyre::Result;
use repy::ebook::{Ebook, Epub};

#[test]
fn test_epub_loading() -> Result<(), Box<dyn std::error::Error>> {
    let epub_path = "books/meditations.epub";
    println!("Loading EPUB: {}", epub_path);

    // Create and initialize EPUB reader
    let mut epub = Epub::new(epub_path);
    epub.initialize()?;

    // Print metadata
    println!("\n=== METADATA ===");
    let metadata = epub.get_meta();
    println!("Title: {:?}", metadata.title);
    println!("Creator: {:?}", metadata.creator);
    println!("Description: {:?}", metadata.description);
    println!("Language: {:?}", metadata.language);
    println!("Publisher: {:?}", metadata.publisher);
    println!("Date: {:?}", metadata.date);

    // Print table of contents
    println!("\n=== TABLE OF CONTENTS ===");
    let toc = epub.toc_entries();
    println!("Found {} chapters:", toc.len());
    for (i, entry) in toc.iter().enumerate() {
        println!("{:2}: {} (content_index: {})", i, entry.label, entry.content_index);
    }

    // Print content list
    println!("\n=== CONTENT LIST ===");
    let contents = epub.contents().clone();
    println!("Found {} content items:", contents.len());
    for (i, content_id) in contents.iter().take(10).enumerate() {
        println!("  {}: {}", i, content_id);
    }
    if contents.len() > 10 {
        println!("  ... and {} more", contents.len() - 10);
    }

    // Try to get raw text for the first chapter
    if !contents.is_empty() {
        println!("\n=== FIRST CHAPTER (RAW) ===");
        let first_chapter = contents[0].clone();
        match epub.get_raw_text(&first_chapter) {
            Ok(raw_html) => {
                println!("Raw HTML length: {} characters", raw_html.len());

                // Show first 500 characters of raw HTML
                let preview = raw_html.chars().take(500).collect::<String>();
                println!("Raw HTML preview:\n{}", preview);
                if raw_html.len() > 500 {
                    println!("...(truncated)");
                }
            }
            Err(e) => println!("Error getting raw text: {}", e),
        }
    }

    // Try to parse the first chapter
    if !contents.is_empty() {
        println!("\n=== FIRST CHAPTER (PARSED) ===");
        let first_chapter = contents[0].clone();
        match epub.get_parsed_content(&first_chapter, 80, 0) {
            Ok(parsed) => {
                println!("Parsed text has {} lines", parsed.text_lines.len());
                println!("Found {} images", parsed.image_maps.len());
                println!("Found {} sections", parsed.section_rows.len());
                println!("Found {} formatting entries", parsed.formatting.len());

                // Show first 20 lines of parsed text
                println!("\nFirst 20 lines of parsed text:");
                for (i, line) in parsed.text_lines.iter().take(20).enumerate() {
                    if line.trim().is_empty() {
                        println!("{:2}: [empty]", i);
                    } else {
                        println!("{:2}: {}", i, line);
                    }
                }

                // Show images found
                if !parsed.image_maps.is_empty() {
                    println!("\nImages found:");
                    for (line_num, path) in &parsed.image_maps {
                        println!("  Line {}: {}", line_num, path);
                    }
                }

                // Show sections found
                if !parsed.section_rows.is_empty() {
                    println!("\nSections found:");
                    for (section_id, line_num) in &parsed.section_rows {
                        println!("  Section '{}' at line {}", section_id, line_num);
                    }
                }
            }
            Err(e) => println!("Error parsing content: {}", e),
        }
    }

    // Test parsing multiple chapters
    if contents.len() > 1 {
        println!("\n=== MULTIPLE CHAPTERS TEST ===");
        println!("Parsing first 3 chapters...");

        let all_content = epub.get_all_parsed_content(80)?;
        println!("Successfully parsed {} chapters", all_content.len());

        let mut total_lines = 0;
        for (i, content) in all_content.iter().enumerate() {
            total_lines += content.text_lines.len();
            println!("  Chapter {}: {} lines, {} images",
                i + 1, content.text_lines.len(), content.image_maps.len());
        }
        println!("Total lines across all chapters: {}", total_lines);
    }

    // Cleanup
    epub.cleanup()?;
    println!("\nEPUB test completed successfully!");

    Ok(())
}
