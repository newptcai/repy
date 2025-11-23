use ratatui::{layout::Rect, style::{Modifier, Style}, text::{Line, Span}, widgets::{Block, Borders, Clear, Paragraph}, Frame};

pub struct SearchWindow {
    pub visible: bool,
    pub query: String,
}

impl SearchWindow {
    pub fn new() -> Self {
        Self {
            visible: false,
            query: String::new(),
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    pub fn add_char(&mut self, c: char) {
        self.query.push(c);
    }

    pub fn remove_char(&mut self) {
        self.query.pop();
    }

    pub fn clear(&mut self) {
        self.query.clear();
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let popup_area = Rect::new(
            area.x + 2,
            area.y + area.height / 2 - 2,
            area.width - 4,
            5,
        );

        frame.render_widget(Clear, popup_area);

        let content = vec![
            Line::from("Search:"),
            Line::from(format!("/{}", self.query)),
            Line::from(""),
            Line::from(Span::styled("Enter: search  Esc: cancel", Style::default().add_modifier(Modifier::DIM))),
        ];

        let paragraph = Paragraph::new(content)
            .block(Block::default().title("Search").borders(Borders::ALL));

        frame.render_widget(paragraph, popup_area);
    }
}