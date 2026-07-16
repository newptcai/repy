use crate::{
    opds::{Availability, Feed},
    settings::OpdsCatalogConfig,
    theme::Theme,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::Line,
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

pub struct OpdsWindow;
impl OpdsWindow {
    fn area(area: Rect) -> Rect {
        Rect::new(
            area.x + area.width / 8,
            area.y + area.height / 10,
            area.width * 3 / 4,
            area.height * 4 / 5,
        )
    }
    pub fn catalogs(
        frame: &mut Frame,
        area: Rect,
        catalogs: &[OpdsCatalogConfig],
        selected: usize,
        theme: &Theme,
    ) {
        let area = Self::area(area);
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title("OPDS Catalogs")
            .title_bottom(" Enter open · q Library ")
            .borders(Borders::ALL)
            .style(theme.base_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let items: Vec<_> = catalogs
            .iter()
            .enumerate()
            .map(|(i, c)| {
                ListItem::new(Line::from(c.name.clone())).style(if i == selected {
                    Style::default()
                        .bg(theme.highlight_bg)
                        .fg(theme.highlight_fg)
                } else {
                    theme.base_style()
                })
            })
            .collect();
        frame.render_widget(List::new(items), inner);
    }
    pub fn feed(
        frame: &mut Frame,
        area: Rect,
        feed: Option<&Feed>,
        selected: usize,
        format_index: usize,
        loading: bool,
        downloading: bool,
        downloaded_bytes: u64,
        total_bytes: Option<u64>,
        error: Option<&str>,
        details: bool,
        theme: &Theme,
    ) {
        let area = Self::area(area);
        frame.render_widget(Clear, area);
        let title = feed
            .map(|f| f.title.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("OPDS");
        let block=Block::default().title(format!(" {title} ")).title_bottom(" Enter open/download · / search · [/] pages · f format · c details · h back · q Library ").borders(Borders::ALL).style(theme.base_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if loading {
            let tick = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                / 120;
            let label = if downloading {
                let human = |bytes: u64| {
                    if bytes >= 1024 * 1024 {
                        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
                    } else {
                        format!("{:.0} KiB", bytes as f64 / 1024.0)
                    }
                };
                if let Some(total) = total_bytes.filter(|total| *total > 0) {
                    let percent = (downloaded_bytes.saturating_mul(100) / total).min(100);
                    let filled = (percent as usize * 20 / 100).min(20);
                    format!(
                        "Downloading [{:<20}] {:>3}% · {} / {}",
                        "█".repeat(filled),
                        percent,
                        human(downloaded_bytes),
                        human(total)
                    )
                } else {
                    let position = tick as usize % 17;
                    format!(
                        "Downloading [{}] · {}",
                        format!(
                            "{}{}{}",
                            " ".repeat(position),
                            "████",
                            " ".repeat(16 - position)
                        ),
                        human(downloaded_bytes)
                    )
                }
            } else {
                let position = tick as usize % 17;
                format!(
                    "Loading feed [{}]",
                    format!(
                        "{}{}{}",
                        " ".repeat(position),
                        "████",
                        " ".repeat(16 - position)
                    )
                )
            };
            frame.render_widget(
                Paragraph::new(label).style(theme.base_style().fg(theme.muted_fg)),
                inner,
            );
            return;
        }
        if let Some(e) = error {
            frame.render_widget(
                Paragraph::new(format!("Error: {e}"))
                    .wrap(Wrap { trim: false })
                    .style(theme.base_style().fg(theme.warning_fg)),
                inner,
            );
            return;
        }
        let Some(feed) = feed else { return };
        let mut rows = Vec::new();
        for n in &feed.navigation {
            rows.push(format!("› {}", n.title));
        }
        for p in &feed.publications {
            let readable = p.readable_acquisitions();
            let status = if readable.is_empty() {
                "unavailable".into()
            } else {
                let link = readable[format_index % readable.len()];
                link.extension().unwrap_or("book").to_uppercase()
            };
            rows.push(format!(
                "{} — {} [{}]",
                p.title,
                p.authors.join(", "),
                status
            ));
        }
        if rows.is_empty() {
            rows.push("No entries".into())
        }
        let list_height = if details {
            inner.height.saturating_mul(2) / 3
        } else {
            inner.height
        };
        let list_area = Rect::new(inner.x, inner.y, inner.width, list_height);
        let items: Vec<_> = rows
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                ListItem::new(s).style(if i == selected {
                    Style::default()
                        .bg(theme.highlight_bg)
                        .fg(theme.highlight_fg)
                } else {
                    theme.base_style()
                })
            })
            .collect();
        frame.render_widget(List::new(items), list_area);
        if details {
            if let Some(p) = selected
                .checked_sub(feed.navigation.len())
                .and_then(|i| feed.publications.get(i))
            {
                let unavailable = p
                    .acquisitions
                    .iter()
                    .filter(|a| a.availability != Availability::Readable)
                    .count();
                let text = format!(
                    "{}\nAuthors: {}\n{}{}",
                    p.title,
                    p.authors.join(", "),
                    p.summary.as_deref().unwrap_or("No description"),
                    if unavailable > 0 {
                        format!("\n{unavailable} unavailable acquisition(s)")
                    } else {
                        String::new()
                    }
                );
                frame.render_widget(
                    Paragraph::new(text)
                        .wrap(Wrap { trim: false })
                        .style(theme.base_style()),
                    Rect::new(
                        inner.x,
                        inner.y + list_height,
                        inner.width,
                        inner.height - list_height,
                    ),
                );
            }
        }
    }
    pub fn search(frame: &mut Frame, area: Rect, query: &str, theme: &Theme) {
        let w = area.width.min(64);
        let a = Rect::new(
            area.x + (area.width - w) / 2,
            area.y + area.height / 2 - 1,
            w,
            3,
        );
        frame.render_widget(Clear, a);
        frame.render_widget(
            Paragraph::new(format!("{query}█"))
                .block(
                    Block::default()
                        .title("Search catalog")
                        .borders(Borders::ALL),
                )
                .style(theme.base_style()),
            a,
        );
    }
}
