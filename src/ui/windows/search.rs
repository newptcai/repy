use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub struct SearchWindow;

impl SearchWindow {
    pub fn render(frame: &mut Frame, area: Rect, query: &str, results: &[String], selected_index: usize) {
        let popup_area = Rect::new(
            area.x + area.width / 8,
            area.y + area.height / 6,
            area.width * 3 / 4,
            area.height * 2 / 3,
        );

        frame.render_widget(Clear, popup_area);

        let header = Paragraph::new(Line::from(format!("/{}", query)))
            .block(Block::default().title("Search").borders(Borders::ALL))
            .style(Style::default().add_modifier(Modifier::BOLD));

        let header_area = Rect::new(popup_area.x, popup_area.y, popup_area.width, 3);
        frame.render_widget(header, header_area);

        let list_area = Rect::new(popup_area.x, popup_area.y + 3, popup_area.width, popup_area.height - 3);

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

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL));

        frame.render_widget(list, list_area);
    }
}
