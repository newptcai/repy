use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub struct HelpWindow;

const HELP_TEXT: &[&str] = &[
    " Key Bindings:",
    "   k / Up            Line Up",
    "   j / Down          Line Down",
    "   h / Left          Page Up",
    "   l / Right / Space Page Down",
    "   L                 Next Chapter",
    "   H                 Prev Chapter",
    "   g                 Chapter Start",
    "   G                 Chapter End",
    "   Home              Book Start",
    "   End               Book End",
    "",
    " Jump History:",
    "   Ctrl+o            Jump Back",
    "   Ctrl+i/Tab        Jump Forward",
    "",
    " Search:",
    "   /                 Start Search",
    "   n                 Next Hit",
    "   p / N             Previous Hit",
    "",
    " Display:",
    "   + / -             Increase/Decrease Width",
    "   =                 Reset Width",
    "   T                 Toggle Top Bar",
    "",
    " Windows & Tools:",
    "   v                 Visual Mode (Select & Yank)",
    "   t                 Table Of Contents",
    "   m                 Bookmarks",
    "   u                 Links on Page",
    "   o                 Images on Page",
    "   i                 Metadata",
    "   r                 Library (History)",
    "   s                 Settings",
    "   q                 Quit / Close Window",
    "   ?                 Help",
];

impl HelpWindow {
    pub fn get_total_lines() -> usize {
        HELP_TEXT.len()
    }

    pub fn render(frame: &mut Frame, area: Rect, scroll_offset: u16) {
        let help_content: Vec<Line> = HELP_TEXT.iter().map(|&s| Line::from(s)).collect();

        let max_width = help_content.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
        let width = (max_width + 4).min(area.width);
        let height = (help_content.len() as u16 + 2).min(area.height);

        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;
        let popup_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, popup_area);

        let help_paragraph = Paragraph::new(help_content)
            .block(Block::default().title("Help").borders(Borders::ALL))
            .scroll((scroll_offset, 0));

        frame.render_widget(help_paragraph, popup_area);
    }
}
