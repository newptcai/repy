use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub struct HelpWindow;

impl HelpWindow {
    pub fn render(frame: &mut Frame, area: Rect) {
        let help_content = vec![
            Line::from(" Key Bindings:"),
            Line::from("   k / Up            Line Up"),
            Line::from("   j / Down          Line Down"),
            Line::from("   h / Left          Page Up"),
            Line::from("   l / Right / Space Page Down"),
            Line::from("   L                 Next Chapter"),
            Line::from("   H                 Prev Chapter"),
            Line::from("   g                 Chapter Start"),
            Line::from("   G                 Chapter End"),
            Line::from("   Home              Book Start"),
            Line::from("   End               Book End"),
            Line::from(""),
            Line::from(" Jump History:"),
            Line::from("   Ctrl+o            Jump Back"),
            Line::from("   Ctrl+i/Tab        Jump Forward"),
            Line::from(""),
            Line::from(" Display:"),
            Line::from("   + / -             Increase/Decrease Width"),
            Line::from("   =                 Reset Width"),
            Line::from("   T                 Toggle Top Bar"),
            Line::from(""),
            Line::from(" Windows & Tools:"),
            Line::from("   /                 Search"),
            Line::from("   n                 Search Next"),
            Line::from("   p / N             Search Prev"),
            Line::from("   v                 Visual Mode (Select & Yank)"),
            Line::from("   t                 Table Of Contents"),
            Line::from("   m                 Bookmarks"),
            Line::from("   u                 Links on Page"),
            Line::from("   o                 Images on Page"),
            Line::from("   i                 Metadata"),
            Line::from("   r                 Library (History)"),
            Line::from("   s                 Settings"),
            Line::from("   q                 Quit / Close Window"),
            Line::from("   ?                 Help"),
        ];

        let max_width = help_content.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
        let width = (max_width + 4).min(area.width);
        let height = (help_content.len() as u16 + 2).min(area.height);

        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;
        let popup_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, popup_area);

        let help_paragraph = Paragraph::new(help_content)
            .block(Block::default().title("Help").borders(Borders::ALL));

        frame.render_widget(help_paragraph, popup_area);
    }
}
