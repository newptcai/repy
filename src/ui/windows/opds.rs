use crate::{
    opds::{Availability, Feed},
    settings::OpdsCatalogConfig,
    theme::Theme,
};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
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
    /// Number of leading rows to skip so `selected` stays visible in `visible` rows.
    fn scroll_offset(selected: usize, total: usize, visible: usize) -> usize {
        if visible == 0 || total <= visible {
            return 0;
        }
        selected
            .saturating_sub(visible.saturating_sub(1))
            .min(total - visible)
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
        let title = if catalogs.is_empty() {
            " OPDS Catalogs ".to_string()
        } else {
            format!(" OPDS Catalogs · {}/{} ", selected + 1, catalogs.len())
        };
        let block = Block::default()
            .title(title)
            .title_bottom(" Enter open · q Library ")
            .borders(Borders::ALL)
            .style(theme.base_style());
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if catalogs.is_empty() {
            frame.render_widget(
                Paragraph::new(
                    "No OPDS catalogs configured.\n\n\
                     Add entries to \"opds_catalogs\" in configuration.json:\n\n  \
                     { \"name\": \"Project Gutenberg\",\n    \
                     \"url\": \"https://www.gutenberg.org/ebooks.opds/\" }",
                )
                .wrap(Wrap { trim: false })
                .style(theme.base_style().fg(theme.muted_fg)),
                inner,
            );
            return;
        }
        let offset = Self::scroll_offset(selected, catalogs.len(), inner.height as usize);
        let items: Vec<_> = catalogs
            .iter()
            .enumerate()
            .skip(offset)
            .take(inner.height as usize)
            .map(|(i, c)| {
                let line = if i == selected {
                    Line::from(format!("{}  {}", c.name, c.url)).style(
                        Style::default()
                            .bg(theme.highlight_bg)
                            .fg(theme.highlight_fg),
                    )
                } else {
                    Line::from(vec![
                        Span::raw(c.name.clone()),
                        Span::styled(
                            format!("  {}", c.url),
                            theme.base_style().fg(theme.muted_fg),
                        ),
                    ])
                };
                ListItem::new(line)
            })
            .collect();
        frame.render_widget(List::new(items), inner);
    }
    #[allow(clippy::too_many_arguments)]
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
        page: usize,
        theme: &Theme,
    ) {
        let area = Self::area(area);
        frame.render_widget(Clear, area);
        let title = feed
            .map(|f| f.title.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("OPDS");
        let total_entries = feed
            .map(|f| f.navigation.len() + f.publications.len())
            .unwrap_or(0);
        let top_title = if !loading && error.is_none() && total_entries > 0 {
            // Prefer the catalog-wide position (OpenSearch totalResults +
            // startIndex) so the counter keeps moving across [/] pages;
            // fall back to a per-page counter with an explicit page number.
            feed.filter(|f| f.navigation.is_empty())
                .and_then(|f| {
                    let total = f.pagination.total_results.filter(|t| *t > 0)?;
                    let start = f.pagination.start_index.unwrap_or(1).max(1);
                    Some(format!(" {title} · {}/{total} ", start + selected as u64))
                })
                .unwrap_or_else(|| {
                    let paginated = feed.is_some_and(|f| {
                        f.pagination.next.is_some() || f.pagination.previous.is_some()
                    });
                    if paginated || page > 1 {
                        format!(
                            " {title} · {}/{} · page {page} ",
                            selected + 1,
                            total_entries
                        )
                    } else {
                        format!(" {title} · {}/{} ", selected + 1, total_entries)
                    }
                })
        } else {
            format!(" {title} ")
        };
        let full_hint = " Enter open/download · / search · [/] pages · f format · c details · h back · q Library ";
        let short_hint = " ⏎ open · / search · [/] page · f fmt · c details · h back";
        let hint = if area.width as usize >= full_hint.chars().count() + 2 {
            full_hint
        } else {
            short_hint
        };
        let mut block = Block::default()
            .title(top_title)
            .title_bottom(hint)
            .borders(Borders::ALL)
            .style(theme.base_style());
        if let Some(feed) = feed.filter(|_| !loading && error.is_none()) {
            let mut page_hint = String::new();
            if feed.pagination.previous.is_some() {
                page_hint.push_str("[ prev");
            }
            if feed.pagination.next.is_some() {
                if !page_hint.is_empty() {
                    page_hint.push_str(" · ");
                }
                page_hint.push_str("next ]");
            }
            if !page_hint.is_empty() {
                block = block.title_top(Line::from(format!(" {page_hint} ")).right_aligned());
            }
        }
        let inner = block.inner(area);
        frame.render_widget(block, area);
        if loading {
            let verb = if downloading {
                "Downloading"
            } else {
                "Loading feed"
            };
            let human = |bytes: u64| {
                if bytes >= 1024 * 1024 {
                    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
                } else {
                    format!("{:.0} KiB", bytes as f64 / 1024.0)
                }
            };
            let label = if let Some(total) = total_bytes.filter(|total| *total > 0) {
                // Real progress: bytes received over the Content-Length.
                let percent = (downloaded_bytes.saturating_mul(100) / total).min(100);
                let filled = (percent as usize * 16 / 100).min(16);
                format!(
                    "{verb} [{:<16}] {:>3}% · {} / {}",
                    "█".repeat(filled),
                    percent,
                    human(downloaded_bytes),
                    human(total)
                )
            } else {
                // No Content-Length: animate a marquee but still show the
                // real byte count once data starts arriving.
                let tick = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    / 120;
                let position = tick as usize % 17;
                let bar = format!("{}████{}", " ".repeat(position), " ".repeat(16 - position));
                if downloaded_bytes > 0 {
                    format!("{verb} [{bar}] · {}", human(downloaded_bytes))
                } else {
                    format!("{verb} [{bar}]")
                }
            };
            let row = Rect::new(
                inner.x,
                inner.y + inner.height / 2,
                inner.width,
                inner.height.min(1),
            );
            frame.render_widget(
                Paragraph::new(label)
                    .alignment(Alignment::Center)
                    .style(theme.base_style().fg(theme.muted_fg)),
                row,
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
        let selected_style = Style::default()
            .bg(theme.highlight_bg)
            .fg(theme.highlight_fg);
        let mut rows: Vec<Line> = Vec::new();
        for (i, n) in feed.navigation.iter().enumerate() {
            let text = format!("› {}", n.title);
            rows.push(if i == selected {
                Line::from(text).style(selected_style)
            } else {
                Line::from(text).style(theme.base_style().fg(theme.info_fg))
            });
        }
        for (i, p) in feed.publications.iter().enumerate() {
            let index = feed.navigation.len() + i;
            let readable = p.readable_acquisitions();
            let tag = if readable.is_empty() {
                "unavailable".to_string()
            } else {
                let link = readable[format_index % readable.len()];
                let ext = link.extension().unwrap_or("book").to_uppercase();
                if readable.len() > 1 {
                    format!(
                        "{ext} {}/{}",
                        format_index % readable.len() + 1,
                        readable.len()
                    )
                } else {
                    ext
                }
            };
            let authors = if p.authors.is_empty() {
                String::new()
            } else {
                format!(" — {}", p.authors.join(", "))
            };
            rows.push(if index == selected {
                Line::from(format!("{}{} [{}]", p.title, authors, tag)).style(selected_style)
            } else {
                Line::from(vec![
                    Span::raw(p.title.clone()),
                    Span::styled(authors, theme.base_style().fg(theme.muted_fg)),
                    Span::styled(
                        format!(" [{tag}]"),
                        theme.base_style().fg(if readable.is_empty() {
                            theme.warning_fg
                        } else {
                            theme.muted_fg
                        }),
                    ),
                ])
            });
        }
        if rows.is_empty() {
            rows.push(Line::from("No entries").style(theme.base_style().fg(theme.muted_fg)));
        }
        let list_height = if details {
            inner.height.saturating_mul(2) / 3
        } else {
            inner.height
        };
        let list_area = Rect::new(inner.x, inner.y, inner.width, list_height);
        let offset = Self::scroll_offset(selected, rows.len(), list_height as usize);
        let items: Vec<_> = rows
            .into_iter()
            .skip(offset)
            .take(list_height as usize)
            .map(ListItem::new)
            .collect();
        frame.render_widget(List::new(items), list_area);
        if details
            && let Some(p) = selected
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
            let details_area = Rect::new(
                inner.x,
                inner.y + list_height,
                inner.width,
                inner.height - list_height,
            );
            let details_block = Block::default()
                .borders(Borders::TOP)
                .title(" Details · c close ")
                .style(theme.base_style());
            let details_inner = details_block.inner(details_area);
            frame.render_widget(details_block, details_area);
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .style(theme.base_style()),
                details_inner,
            );
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
                        .title(" Search catalog ")
                        .title_bottom(" Enter search · Esc cancel ")
                        .borders(Borders::ALL),
                )
                .style(theme.base_style()),
            a,
        );
    }
}
