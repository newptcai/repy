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
            Line::from("   k  Scroll Up"),
            Line::from("   j  Scroll Down"),
            Line::from("   h  Page Up"),
            Line::from("   l  Page Down"),
            Line::from("   L  Next Chapter"),
            Line::from("   H  Prev Chapter"),
            Line::from("   g  Beginning Of Ch"),
            Line::from("   G  End Of Ch"),
            Line::from("   -  Shrink"),
            Line::from("   +  Enlarge"),
            Line::from("   =  Set Width"),
            Line::from("   M  Metadata"),
            Line::from("   d  Define Word"),
            Line::from("   t  Table Of Contents"),
            Line::from("   f  Follow"),
            Line::from("   o  Open Image"),
            Line::from("   /  Regex Search"),
            Line::from("   s  Show Hide Progress"),
            Line::from("   m  Mark Position"),
            Line::from("   `  Jump To Position"),
            Line::from("   b  Add Bookmark"),
            Line::from("   B  Show Bookmarks"),
            Line::from("   u  Links On Page (Enter follow)"),
            Line::from("   q  Quit"),
            Line::from("   ?  Help"),
            Line::from("   c  Switch Color"),
            Line::from("   !  T T S Toggle"),
            Line::from("   D  Double Spread Toggle"),
            Line::from("   r  Library (history; 'd' deletes entry)"),
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
