use ratatui::{
    Frame,
    layout::Rect,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub struct DictionaryWindow;

impl DictionaryWindow {
    pub fn render(frame: &mut Frame, area: Rect, word: &str, definition: &str, scroll_offset: u16) {
        let popup_area = Self::centered_popup_area(area, 70, 80);
        frame.render_widget(Clear, popup_area);

        let title = if word.is_empty() {
            "Dictionary".to_string()
        } else {
            format!("Dictionary: {word}")
        };

        let paragraph = Paragraph::new(definition)
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: false })
            .scroll((scroll_offset, 0));

        frame.render_widget(paragraph, popup_area);
    }

    fn centered_popup_area(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
        let width = (area.width * width_percent) / 100;
        let height = (area.height * height_percent) / 100;
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;

        Rect::new(x, y, width, height)
    }
}
