use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::models::{ReadingStatistics, ReadingStatsTotals};
use crate::statistics::format_duration;
use crate::theme::Theme;
use crate::ui::windows::centered_popup_area;

pub struct StatisticsWindow;

impl StatisticsWindow {
    pub fn render(frame: &mut Frame, area: Rect, stats: &ReadingStatistics, theme: &Theme) {
        let popup_area = centered_popup_area(area, 72, 68);
        let block = Block::default()
            .title("Statistics")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.info_fg))
            .style(theme.base_style());

        let mut lines = Vec::new();
        lines.push(Line::from(vec![Span::styled(
            Self::book_heading(stats),
            theme.base_style().add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));
        lines.extend(Self::totals_lines("This book", &stats.book, theme));
        if let Some(minutes) = stats.estimated_chapter_minutes_left {
            lines.push(Line::from(format!(
                "  Chapter left: {}",
                Self::format_minutes(minutes)
            )));
        }
        if let Some(minutes) = stats.estimated_book_minutes_left {
            lines.push(Line::from(format!(
                "  Book left:    {}",
                Self::format_minutes(minutes)
            )));
        }
        lines.push(Line::from(""));
        lines.extend(Self::totals_lines("All books", &stats.global, theme));
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "  Current streak: {}",
            Self::format_days(stats.current_streak_days)
        )));
        lines.push(Line::from(format!(
            "  Longest streak: {}",
            Self::format_days(stats.longest_streak_days)
        )));
        lines.push(Line::from(""));
        lines.push(Line::from("  Esc/q closes"));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: true })
            .style(theme.base_style());

        frame.render_widget(Clear, popup_area);
        frame.render_widget(paragraph, popup_area);
    }

    fn book_heading(stats: &ReadingStatistics) -> String {
        match (&stats.book_title, &stats.book_author) {
            (Some(title), Some(author)) => format!("{title} - {author}"),
            (Some(title), None) => title.clone(),
            (None, Some(author)) => author.clone(),
            (None, None) => "Current book".to_string(),
        }
    }

    fn totals_lines(label: &str, totals: &ReadingStatsTotals, theme: &Theme) -> Vec<Line<'static>> {
        vec![
            Line::from(vec![Span::styled(
                label.to_string(),
                theme.base_style().add_modifier(Modifier::BOLD),
            )]),
            Line::from(format!("  Time:     {}", format_duration(totals.seconds))),
            Line::from(format!("  Words:    {}", totals.words)),
            Line::from(format!("  Rows:     {}", totals.rows)),
            Line::from(format!("  Sessions: {}", totals.sessions)),
            Line::from(format!(
                "  WPM:      {}",
                totals
                    .words_per_minute()
                    .map(|wpm| format!("{wpm:.0}"))
                    .unwrap_or_else(|| "N/A".to_string())
            )),
        ]
    }

    fn format_minutes(minutes: i64) -> String {
        let minutes = minutes.max(0);
        if minutes >= 60 {
            format!("{}h {}m", minutes / 60, minutes % 60)
        } else {
            format!("{minutes}m")
        }
    }

    fn format_days(days: usize) -> String {
        match days {
            0 => "0 days".to_string(),
            1 => "1 day".to_string(),
            n => format!("{n} days"),
        }
    }
}
