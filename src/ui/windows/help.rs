use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub struct HelpWindow;

impl HelpWindow {
    pub fn render(frame: &mut Frame, area: Rect) {
        let popup_area = Self::centered_popup_area(area, 60, 70);

        frame.render_widget(Clear, popup_area);

        let help_content = vec![
            Line::from(" Key Bindings:"),
            Line::from("   k / Up      Line Up"),
            Line::from("   j / Down    Line Down"),
            Line::from("   h / Left    Page Up"),
            Line::from("   l / Right   Page Down"),
            Line::from("   Space / f   Page Down"),
            Line::from("   b           Page Up"),
            Line::from("   L           Next Chapter"),
            Line::from("   H           Prev Chapter"),
            Line::from("   g           Chapter Start"),
            Line::from("   G           Chapter End"),
            Line::from("   Home        Book Start"),
            Line::from("   End         Book End"),
            Line::from(""),
            Line::from(" Jump History:"),
            Line::from("   Ctrl+o      Jump Back"),
            Line::from("   Ctrl+i/Tab  Jump Forward"),
            Line::from(""),
            Line::from(" Display:"),
            Line::from("   + / -       Increase/Decrease Width"),
            Line::from("   =           Reset Width"),
            Line::from("   T           Toggle Top Bar"),
            Line::from(""),
            Line::from(" Windows & Tools:"),
            Line::from("   /  Search"),
            Line::from("   v  Visual Mode (Select & Yank)"),
            Line::from("   t  Table Of Contents"),
            Line::from("   m  Bookmarks"),
            Line::from("   u  Links on Page"),
            Line::from("   o  Images on Page"),
            Line::from("   i  Metadata"),
            Line::from("   r  Library (History)"),
            Line::from("   s  Settings"),
            Line::from("   q  Quit / Close Window"),
            Line::from("   ?  Help"),
        ];

        let help_paragraph = Paragraph::new(help_content)
            .block(Block::default().title("Help").borders(Borders::ALL));

        frame.render_widget(help_paragraph, popup_area);
    }

    fn centered_popup_area(area: Rect, width_percent: u16, height_percent: u16) -> Rect {
        let width = (area.width * width_percent) / 100;
        let height = (area.height * height_percent) / 100;
        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;

        Rect::new(x, y, width, height)
    }
}
