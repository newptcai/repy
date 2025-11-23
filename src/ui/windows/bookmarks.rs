use ratatui::{layout::Rect, widgets::{Block, Borders, Clear, Paragraph}, Frame};

pub struct BookmarksWindow {
    pub visible: bool,
}

impl BookmarksWindow {
    pub fn new() -> Self {
        Self { visible: false }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let popup_area = Rect::new(
            area.x + area.width / 4,
            area.y + area.height / 4,
            area.width / 2,
            area.height / 2,
        );

        frame.render_widget(Clear, popup_area);

        let paragraph = Paragraph::new("Bookmarks management\n\nTODO: Implement bookmarks UI")
            .block(Block::default().title("Bookmarks").borders(Borders::ALL));

        frame.render_widget(paragraph, popup_area);
    }
}