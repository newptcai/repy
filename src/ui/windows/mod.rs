pub mod bookmarks;
pub mod dictionary;
pub mod help;
pub mod images;
pub mod library;
pub mod links;
pub mod metadata;
pub mod search;
pub mod settings;
pub mod toc;

use ratatui::layout::Rect;

/// Compute a centered popup area within the given area.
pub fn centered_popup_area(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
    let width = (area.width * width_percent) / 100;
    let height = (area.height * height_percent) / 100;
    let x = area.x + (area.width - width) / 2;
    let y = area.y + (area.height - height) / 2;

    Rect::new(x, y, width, height)
}
