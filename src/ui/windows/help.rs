use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub struct HelpWindow {
    pub visible: bool,
}

impl HelpWindow {
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

        let popup_area = self.centered_popup_area(area, 60, 70);

        frame.render_widget(Clear, popup_area);

        let help_content = vec![
            Line::from(Span::styled(
                "Keyboard Shortcuts",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Navigation:"),
            Line::from("  j, ↓     - Move down"),
            Line::from("  k, ↑     - Move up"),
            Line::from("  h, ←     - Move left"),
            Line::from("  l, →     - Move right"),
            Line::from("  Space, f - Page down"),
            Line::from("  b        - Page up"),
            Line::from("  g        - Go to start"),
            Line::from("  G        - Go to end"),
            Line::from(""),
            Line::from("Chapters:"),
            Line::from("  Ctrl+n   - Next chapter"),
            Line::from("  Ctrl+p   - Previous chapter"),
            Line::from(""),
            Line::from("Search:"),
            Line::from("  /        - Start search"),
            Line::from("  n        - Next search result"),
            Line::from("  N        - Previous search result"),
            Line::from(""),
            Line::from("Windows:"),
            Line::from("  ?        - Toggle this help"),
            Line::from("  t        - Table of contents"),
            Line::from("  m        - Bookmarks"),
            Line::from("  i        - Book metadata"),
            Line::from(""),
            Line::from("Other:"),
            Line::from("  q        - Quit window or application"),
            Line::from("  1-9      - Count prefix for commands (e.g., 5j)"),
            Line::from(""),
            Line::from(Span::styled("Press any key to close", Style::default().add_modifier(Modifier::ITALIC))),
        ];

        let help_paragraph = Paragraph::new(help_content)
            .block(Block::default().title("Help").borders(Borders::ALL));

        frame.render_widget(help_paragraph, popup_area);
    }

    fn centered_popup_area(&self, area: Rect, width_percent: u16, height_percent: u16) -> Rect {
        let width = (area.width * width_percent) / 100;
        let height = (area.height * height_percent) / 100;
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;

        Rect::new(x, y, width, height)
    }
}