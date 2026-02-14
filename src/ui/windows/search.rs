use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

pub struct SearchWindow;

impl SearchWindow {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        query: &str,
        results: &[String],
        selected_index: usize,
    ) {
        let popup_area = Self::centered_popup_area(area, 60, 70);

        frame.render_widget(Clear, popup_area);

        let header = Paragraph::new(Line::from(format!("/{}", query)))
            .block(Block::default().title("Search").borders(Borders::ALL))
            .style(Style::default().add_modifier(Modifier::BOLD));

        let header_area = Rect::new(popup_area.x, popup_area.y, popup_area.width, 3);
        frame.render_widget(header, header_area);

        // Set cursor position after / and query text
        frame.set_cursor_position((
            header_area.x + query.len() as u16 + 2,
            header_area.y + 1,
        ));

        let list_area = Rect::new(
            popup_area.x,
            popup_area.y + 3,
            popup_area.width,
            popup_area.height - 3,
        );

        if results.is_empty() {
            let empty = Paragraph::new("No matches yet")
                .style(Style::default().fg(Color::DarkGray))
                .block(Block::default().borders(Borders::ALL));
            frame.render_widget(empty, list_area);
            return;
        }

        let items: Vec<ListItem> = results
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let style = if i == selected_index {
                    Style::default().bg(Color::Blue).fg(Color::White)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(entry.clone())).style(style)
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL));

        frame.render_widget(list, list_area);
    }

    fn centered_popup_area(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
        let width = (area.width * width_percent) / 100;
        let height = (area.height * height_percent) / 100;
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;

        Rect::new(x, y, width, height)
    }
}
