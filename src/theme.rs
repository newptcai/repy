use ratatui::style::Color;
use serde::{Deserialize, Serialize};

/// Active color theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColorTheme {
    #[default]
    Default,
    Dark,
    Light,
}

impl ColorTheme {
    /// Cycle to the next theme: Default -> Dark -> Light -> Default
    pub fn next(self) -> Self {
        match self {
            ColorTheme::Default => ColorTheme::Dark,
            ColorTheme::Dark => ColorTheme::Light,
            ColorTheme::Light => ColorTheme::Default,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ColorTheme::Default => "default (terminal)",
            ColorTheme::Dark => "dark (Gruvbox)",
            ColorTheme::Light => "light (Gruvbox)",
        }
    }
}

/// Semantic color palette resolved for the active theme
pub struct Theme {
    /// Main text foreground; None means terminal default
    pub text_fg: Option<Color>,
    /// Main text background; None means terminal default
    pub text_bg: Option<Color>,
    /// Highlighted-item text color (selected rows in popup lists)
    pub highlight_fg: Color,
    /// Highlighted-item background color
    pub highlight_bg: Color,
    /// Search-match text color
    pub search_fg: Color,
    /// Search-match background color
    pub search_bg: Color,
    /// Info message color
    pub info_fg: Color,
    /// Warning / hint message color
    pub warning_fg: Color,
    /// Error message color
    pub error_fg: Color,
    /// Muted / secondary text (line numbers, empty-state text)
    pub muted_fg: Color,
    /// External-link indicator color
    pub external_link_fg: Color,
}

impl Theme {
    /// Returns a base Style applying the theme's text fg/bg (for popup blocks).
    pub fn base_style(&self) -> ratatui::style::Style {
        let mut style = ratatui::style::Style::default();
        if let Some(fg) = self.text_fg {
            style = style.fg(fg);
        }
        if let Some(bg) = self.text_bg {
            style = style.bg(bg);
        }
        style
    }

    pub fn for_color_theme(theme: ColorTheme) -> Self {
        match theme {
            ColorTheme::Default => Self::default_theme(),
            ColorTheme::Dark => Self::dark_theme(),
            ColorTheme::Light => Self::light_theme(),
        }
    }

    fn default_theme() -> Self {
        Self {
            text_fg: None,
            text_bg: None,
            highlight_fg: Color::White,
            highlight_bg: Color::Blue,
            search_fg: Color::Black,
            search_bg: Color::Yellow,
            info_fg: Color::Blue,
            warning_fg: Color::Yellow,
            error_fg: Color::Red,
            muted_fg: Color::DarkGray,
            external_link_fg: Color::Yellow,
        }
    }

    /// Gruvbox Dark — https://github.com/morhetz/gruvbox
    fn dark_theme() -> Self {
        Self {
            text_fg: Some(Color::Rgb(235, 219, 178)),   // #ebdbb2  fg1
            text_bg: Some(Color::Rgb(40, 40, 40)),      // #282828  bg0
            highlight_fg: Color::Rgb(40, 40, 40),       // #282828  bg0
            highlight_bg: Color::Rgb(131, 165, 152),    // #83a598  bright-aqua
            search_fg: Color::Rgb(40, 40, 40),          // #282828
            search_bg: Color::Rgb(250, 189, 47),        // #fabd2f  bright-yellow
            info_fg: Color::Rgb(131, 165, 152),         // #83a598  bright-aqua
            warning_fg: Color::Rgb(250, 189, 47),       // #fabd2f  bright-yellow
            error_fg: Color::Rgb(251, 73, 52),          // #fb4934  bright-red
            muted_fg: Color::Rgb(146, 131, 116),        // #928374  gray
            external_link_fg: Color::Rgb(254, 128, 25), // #fe8019  bright-orange
        }
    }

    /// Gruvbox Light — https://github.com/morhetz/gruvbox
    fn light_theme() -> Self {
        Self {
            text_fg: Some(Color::Rgb(60, 56, 54)),      // #3c3836  fg1
            text_bg: Some(Color::Rgb(251, 241, 199)),   // #fbf1c7  bg0
            highlight_fg: Color::Rgb(251, 241, 199),    // #fbf1c7  bg0
            highlight_bg: Color::Rgb(7, 102, 120),      // #076678  dark-aqua
            search_fg: Color::Rgb(251, 241, 199),       // #fbf1c7
            search_bg: Color::Rgb(181, 118, 20),        // #b57614  dark-yellow
            info_fg: Color::Rgb(7, 102, 120),           // #076678  dark-aqua
            warning_fg: Color::Rgb(181, 118, 20),       // #b57614  dark-yellow
            error_fg: Color::Rgb(157, 0, 6),            // #9d0006  dark-red
            muted_fg: Color::Rgb(124, 111, 100),        // #7c6f64  gray
            external_link_fg: Color::Rgb(175, 58, 3),   // #af3a03  dark-orange
        }
    }
}
