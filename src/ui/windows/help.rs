use ratatui::{
    Frame,
    layout::Rect,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

pub struct HelpWindow;

const HELP_TEXT: &[&str] = &[
    " Key Bindings:",
    "   k / Up            Line Up",
    "   j / Down          Line Down",
    "   h / Left          Page Up",
    "   l / Right / Space Page Down",
    "   Ctrl+u            Half Page Up",
    "   Ctrl+d            Half Page Down",
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
    "   v                 Visual Mode",
    "   t                 Table Of Contents",
    "   m                 Bookmarks",
    "   u                 Links on Page",
    "   o                 Images on Page",
    "   i                 Metadata",
    "   r                 Library (History)",
    "   s                 Settings",
    "   r (in Settings)   Reset selected setting",
    "   Dict command      Use %q as query placeholder",
    " Visual Cursor Mode (v):",
    "   hjkl, w/b/e       Move cursor",
    "   v                 Start selection",
    "",
    " Visual Selection Mode (v then v):",
    "   hjkl, b/e         Extend selection",
    "   y                 Yank selection",
    "   d                 Dictionary Lookup",
    "   w                 Wikipedia Summary",
    "   q                 Quit / Close Window",
    "   ?                 Help",
];

impl HelpWindow {
    pub fn get_total_lines() -> usize {
        HELP_TEXT.len()
    }

    pub fn max_scroll_offset(area: Rect) -> u16 {
        let content_len = HELP_TEXT.len();
        let height = (content_len as u16 + 2).min(area.height);
        let inner_height = height.saturating_sub(2) as usize;
        content_len
            .saturating_sub(inner_height)
            .min(u16::MAX as usize) as u16
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_scroll_offset_zero_when_help_fits() {
        let area = Rect::new(0, 0, 120, 80);
        assert_eq!(HelpWindow::max_scroll_offset(area), 0);
    }

    #[test]
    fn max_scroll_offset_positive_when_help_overflows() {
        let area = Rect::new(0, 0, 120, 10);
        assert!(HelpWindow::max_scroll_offset(area) > 0);
    }
}
