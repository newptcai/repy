use crate::models::ReadingStatisticsExport;
use std::fmt::Write;

pub fn format_duration(seconds: i64) -> String {
    let seconds = seconds.max(0);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

pub fn to_json(stats: &ReadingStatisticsExport) -> serde_json::Result<String> {
    serde_json::to_string_pretty(stats)
}

pub fn to_markdown(stats: &ReadingStatisticsExport) -> String {
    let mut out = String::new();
    writeln!(out, "# Reading Statistics").unwrap();
    writeln!(out, "\n## Global Summary").unwrap();
    writeln!(
        out,
        "\n- Time: {}",
        format_duration(stats.global.total.seconds)
    )
    .unwrap();
    writeln!(out, "- Words: {}", stats.global.total.words).unwrap();
    writeln!(out, "- Rows: {}", stats.global.total.rows).unwrap();
    writeln!(out, "- Sessions: {}", stats.global.total.sessions).unwrap();
    writeln!(
        out,
        "- WPM: {}",
        stats
            .global
            .wpm
            .map(|wpm| format!("{wpm:.0}"))
            .unwrap_or_else(|| "N/A".to_string())
    )
    .unwrap();
    writeln!(
        out,
        "- Current streak: {} days",
        stats.global.current_streak_days
    )
    .unwrap();
    writeln!(out, "\n## Books\n").unwrap();
    writeln!(out, "| Title | Time | Words | WPM | Last read |").unwrap();
    writeln!(out, "| --- | ---: | ---: | ---: | --- |").unwrap();
    for book in &stats.books {
        let title = book
            .title
            .as_deref()
            .unwrap_or(&book.book_id)
            .replace('|', "\\|");
        let wpm = book
            .total
            .words_per_minute()
            .map(|value| format!("{value:.0}"))
            .unwrap_or_else(|| "N/A".to_string());
        let last_read = book.last_read.get(..10).unwrap_or(&book.last_read);
        writeln!(
            out,
            "| {title} | {} | {} | {wpm} | {last_read} |",
            format_duration(book.total.seconds),
            book.total.words
        )
        .unwrap();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{BookReadingStatistics, GlobalReadingStatistics, ReadingStatsTotals};

    fn fabricated_stats() -> ReadingStatisticsExport {
        ReadingStatisticsExport {
            global: GlobalReadingStatistics {
                total: ReadingStatsTotals {
                    seconds: 5_400,
                    rows: 42,
                    words: 900,
                    sessions: 2,
                },
                wpm: Some(10.0),
                current_streak_days: 3,
            },
            books: vec![BookReadingStatistics {
                book_id: "book-1".into(),
                title: Some("The Book".into()),
                author: Some("A. Writer".into()),
                total: ReadingStatsTotals {
                    seconds: 5_400,
                    rows: 42,
                    words: 900,
                    sessions: 2,
                },
                wpm: Some(10.0),
                first_read: "2026-07-18T10:00:00+00:00".into(),
                last_read: "2026-07-19T11:30:00+00:00".into(),
            }],
        }
    }

    #[test]
    fn json_contains_global_and_book_fields() {
        let value: serde_json::Value =
            serde_json::from_str(&to_json(&fabricated_stats()).unwrap()).unwrap();
        assert_eq!(value["global"]["seconds"], 5_400);
        assert_eq!(value["global"]["current_streak_days"], 3);
        assert_eq!(value["global"]["wpm"], 10.0);
        assert_eq!(value["books"][0]["author"], "A. Writer");
        assert_eq!(value["books"][0]["wpm"], 10.0);
        assert_eq!(value["books"][0]["first_read"], "2026-07-18T10:00:00+00:00");
    }

    #[test]
    fn markdown_contains_summary_row_and_shared_duration() {
        let output = to_markdown(&fabricated_stats());
        assert!(output.contains("- Time: 1h 30m"));
        assert!(output.contains("| The Book | 1h 30m | 900 | 10 | 2026-07-19 |"));
    }
}
