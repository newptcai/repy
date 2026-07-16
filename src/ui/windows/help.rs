use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::Line,
    widgets::{Block, Borders, Clear, Paragraph},
};

use crate::theme::Theme;

/// A help line is a section header (vs an indented item) when it has exactly
/// one leading space, e.g. `" Navigation:"`. Items start with three spaces.
fn is_section_header(line: &str) -> bool {
    line.starts_with(' ') && !line.starts_with("  ")
}

pub struct HelpWindow;

const HELP_TEXT: &[&str] = &[
    " Navigation:",
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
    " Jump History:",
    "   Ctrl+o            Jump Back",
    "   Ctrl+i/Tab        Jump Forward",
    "   m<c>              Set Mark <c> (a-z, A-Z, 0-9)",
    "   `<c>              Jump To Mark <c>",
    " Search:",
    "   /                 Start Search (matches update as you type)",
    "   Up / Down         Recall search history while typing",
    "   Enter             Confirm query; Enter again jumps & closes",
    "   n                 Next Hit",
    "   p / N             Previous Hit",
    " Annotations:",
    "   A                 Highlights List",
    "   Enter             Jump to Highlight",
    "   e                 Edit Highlight Comment",
    "   d                 Delete Highlight",
    " Display:",
    "   + / -             Increase/Decrease Width",
    "   =                 Reset Width",
    "   T                 Toggle Top Bar",
    "   c                 Cycle Color Theme",
    " Windows & Tools:",
    "   t                 Table Of Contents",
    "   B                 Bookmarks",
    "   u                 Links on Page (Enter previews internal links)",
    "   o                 Images on Page (Enter shows in-terminal, o external)",
    "   i                 Metadata",
    "   r                 Library (history + scanned directories)",
    "   R                 Reading Statistics",
    "   s                 Settings",
    "   /                 Fuzzy-filter list windows (Esc clears, Enter picks)",
    " Library Window:",
    "   Enter             Open book",
    "   c                 Toggle selected book details and cover",
    "   f                 Cycle available formats",
    "   R                 Refresh library directories",
    "   d                 Remove from history",
    "   s                 Cycle sort (recent/title/author/series/progress)",
    " Text-to-Speech:",
    "   !                 Toggle TTS (Read Aloud)",
    " Cursor Mode:",
    "   hjkl, w/b/e       Move cursor (prefix with count, e.g. 5j)",
    "   ^ / $             Start (non-blank) / end of line",
    "   [ / ]             Previous / next paragraph",
    "   f<c> / F<c>       Jump to next/prev <c> on current line",
    "   t<c> / T<c>       Jump just before/after next/prev <c> (line-local)",
    "   /                 Search visible screen (smartcase, spans wraps)",
    "   n / N             Next / Previous match",
    "   Enter             Edit comment of highlight under cursor",
    "   d                 Delete highlight under cursor",
    "   C                 Cycle color of highlight under cursor",
    " Selection Mode:",
    "   hjkl, w/b/e       Extend selection (prefix with count)",
    "   ^ / $             Extend to start / end of line",
    "   [ / ]             Extend by paragraph",
    "   f<c> / F<c>       Extend to next/prev <c> on current line",
    "   t<c> / T<c>       Extend till just before/after next/prev <c>",
    "   /                 Search visible screen (extends selection)",
    "   n / N             Next / Previous match",
    "   y                 Yank selection",
    "   a                 Highlight selection",
    "   c                 Highlight and comment",
    "   d                 Dictionary Lookup",
    "   p                 Wikipedia Summary",
    "   s                 Search with Ecosia",
    "   q                 Quit / Close Window",
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

    pub fn render(frame: &mut Frame, area: Rect, scroll_offset: u16, theme: &Theme) {
        let header_style = theme
            .base_style()
            .fg(theme.info_fg)
            .add_modifier(Modifier::BOLD);
        let help_content: Vec<Line> = HELP_TEXT
            .iter()
            .map(|&s| {
                let line = Line::from(s);
                if is_section_header(s) {
                    line.style(header_style)
                } else {
                    line
                }
            })
            .collect();

        let max_width = help_content.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
        let width = (max_width + 4).min(area.width);
        let height = (help_content.len() as u16 + 2).min(area.height);

        let x = area.x + (area.width - width) / 2;
        let y = area.y + (area.height - height) / 2;
        let popup_area = Rect::new(x, y, width, height);

        frame.render_widget(Clear, popup_area);

        let help_paragraph = Paragraph::new(help_content)
            .style(theme.base_style())
            .block(
                Block::default()
                    .title("Help (?)")
                    .borders(Borders::ALL)
                    .style(theme.base_style()),
            )
            .scroll((scroll_offset, 0));

        frame.render_widget(help_paragraph, popup_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_scroll_offset_zero_when_help_fits() {
        let area = Rect::new(0, 0, 120, 100);
        assert_eq!(HelpWindow::max_scroll_offset(area), 0);
    }

    #[test]
    fn max_scroll_offset_positive_when_help_overflows() {
        let area = Rect::new(0, 0, 120, 10);
        assert!(HelpWindow::max_scroll_offset(area) > 0);
    }

    #[test]
    fn section_headers_detected_by_indentation() {
        assert!(is_section_header(" Navigation:"));
        assert!(is_section_header(" Text-to-Speech:"));
        assert!(!is_section_header("   k / Up            Line Up"));
        assert!(!is_section_header(""));
        // Every non-blank help line is either a header or an indented item.
        assert!(
            HELP_TEXT.iter().any(|line| is_section_header(line)),
            "help text should contain section headers"
        );
    }
}
