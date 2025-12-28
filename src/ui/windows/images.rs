use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, ListState},
    Frame,
};

pub struct ImagesWindow;

impl ImagesWindow {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        images: &[(usize, String)],
        selected_index: usize,
    ) {
        let popup_area = Self::centered_popup_area(area, 60, 60);

        frame.render_widget(Clear, popup_area);

        let items: Vec<ListItem> = images
            .iter()
            .map(|(line, src)| {
                let filename = std::path::Path::new(src)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(src);
                ListItem::new(Line::from(format!("Line {}: {}", line + 1, filename)))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title("Images on Page").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );

        let mut state = ListState::default();
        state.select(Some(selected_index));

        frame.render_stateful_widget(list, popup_area, &mut state);
    }

    fn centered_popup_area(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
        let width = (area.width * width_percent) / 100;
        let height = (area.height * height_percent) / 100;
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;

        Rect::new(x, y, width, height)
    }
}
