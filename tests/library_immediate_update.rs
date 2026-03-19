use eyre::Result;
use repy::{ebook::{Ebook, Epub}, models::ReadingState, state::State};
use tempfile::tempdir;

#[test]
fn test_library_immediate_update_on_load() -> Result<(), Box<dyn std::error::Error>> {
    // Create a temporary directory for the database
    let _temp_dir = tempdir()?;
    
    // Initialize the state with the temporary database
    let state = State::new()?;
    
    // Load an EPUB file
    let epub_path = "tests/fixtures/meditations.epub";
    let mut epub = Epub::new(epub_path);
    epub.initialize()?;
    
    // Create a default reading state
    let reading_state = ReadingState::default();
    
    // First, set the reading state (required due to foreign key constraint)
    state.set_last_reading_state(&epub, &reading_state)?;
    
    // Then update the library (this is what happens when a book is loaded)
    state.update_library(&epub, Some(0.0))?;
    
    // Verify that the book is now in the library
    let library_items = state.get_from_history()?;
    
    // Check that the library is not empty
    assert!(!library_items.is_empty(), "Library should not be empty after loading a book");
    
    // Check that our book is in the library
    let found = library_items.iter().any(|item| {
        item.filepath == epub_path && 
        item.title == epub.get_meta().title && 
        item.author == epub.get_meta().creator
    });
    
    assert!(found, "The loaded book should be present in the library");
    
    println!("✓ Library immediate update test passed!");
    println!("  - Book added to library: {}", epub_path);
    println!("  - Library now contains {} items", library_items.len());
    
    Ok(())
}