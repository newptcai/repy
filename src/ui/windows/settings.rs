use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

pub struct SettingsWindow;

impl SettingsWindow {
    pub fn render(frame: &mut Frame, area: Rect, entries: &[String], selected_index: usize) {
        let popup_area = Rect::new(
            area.x + area.width / 6,
            area.y + area.height / 8,
            area.width * 2 / 3,
            area.height * 3 / 4,
        );

        frame.render_widget(Clear, popup_area);
        let block = Block::default().title("Settings").borders(Borders::ALL);
        let inner = block.inner(popup_area);
        frame.render_widget(block, popup_area);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);
        let footer = Paragraph::new("Tips: Enter activate | r reset | Dict cmd uses %q");

        if entries.is_empty() {
            let paragraph =
                Paragraph::new("No settings available").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(paragraph, rows[0]);
            frame.render_widget(footer, rows[1]);
            return;
        }

        let items: Vec<ListItem> = entries
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

        let list = List::new(items);

        frame.render_widget(list, rows[0]);
        frame.render_widget(footer, rows[1]);
    }
}
