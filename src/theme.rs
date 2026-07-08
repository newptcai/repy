use ratatui::style::Color;
use serde::{Deserialize, Serialize};

/// Active color theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ColorTheme {
    #[default]
    Default,
    Dark,
    Light,
    Sepia,
}

impl ColorTheme {
    /// Cycle to the next theme: Default -> Dark -> Light -> Sepia -> Default
    pub fn next(self) -> Self {
        match self {
            ColorTheme::Default => ColorTheme::Dark,
            ColorTheme::Dark => ColorTheme::Light,
            ColorTheme::Light => ColorTheme::Sepia,
            ColorTheme::Sepia => ColorTheme::Default,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ColorTheme::Default => "default (terminal)",
            ColorTheme::Dark => "dark (Gruvbox)",
            ColorTheme::Light => "light (Gruvbox)",
            ColorTheme::Sepia => "sepia (paper)",
        }
    }

    pub fn storage_name(self) -> &'static str {
        match self {
            ColorTheme::Default => "Default",
            ColorTheme::Dark => "Dark",
            ColorTheme::Light => "Light",
            ColorTheme::Sepia => "Sepia",
        }
    }

    pub fn from_storage_name(name: &str) -> Option<Self> {
        match name {
            "Default" | "default" | "default (terminal)" => Some(ColorTheme::Default),
            "Dark" | "dark" | "dark (Gruvbox)" => Some(ColorTheme::Dark),
            "Light" | "light" | "light (Gruvbox)" => Some(ColorTheme::Light),
            "Sepia" | "sepia" | "sepia (paper)" => Some(ColorTheme::Sepia),
            _ => None,
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
    /// Persistent annotation highlight text color
    pub annotation_highlight_fg: Color,
    /// Persistent annotation highlight background color (yellow / default)
    pub annotation_highlight_bg: Color,
    /// Annotation highlight backgrounds for green/blue/pink/purple
    pub annotation_green_bg: Color,
    pub annotation_blue_bg: Color,
    pub annotation_pink_bg: Color,
    pub annotation_purple_bg: Color,
    /// Search-match text color
    pub search_fg: Color,
    /// Search-match background color
    pub search_bg: Color,
    /// Current search hit text color
    pub search_current_fg: Color,
    /// Current search hit background color
    pub search_current_bg: Color,
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
    /// Background color for a persistent annotation highlight of the given color.
    pub fn annotation_bg(&self, color: crate::models::HighlightColor) -> Color {
        use crate::models::HighlightColor::*;
        match color {
            Yellow => self.annotation_highlight_bg,
            Green => self.annotation_green_bg,
            Blue => self.annotation_blue_bg,
            Pink => self.annotation_pink_bg,
            Purple => self.annotation_purple_bg,
        }
    }

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
            ColorTheme::Sepia => Self::sepia_theme(),
        }
    }

    fn default_theme() -> Self {
        Self {
            text_fg: None,
            text_bg: None,
            highlight_fg: Color::White,
            highlight_bg: Color::Blue,
            annotation_highlight_fg: Color::Black,
            annotation_highlight_bg: Color::Rgb(242, 211, 135),
            annotation_green_bg: Color::Rgb(178, 223, 138),
            annotation_blue_bg: Color::Rgb(166, 206, 227),
            annotation_pink_bg: Color::Rgb(244, 194, 194),
            annotation_purple_bg: Color::Rgb(202, 178, 214),
            search_fg: Color::Black,
            search_bg: Color::Rgb(255, 245, 157), // light pastel yellow
            search_current_fg: Color::Black,
            search_current_bg: Color::Rgb(255, 167, 38), // orange
            info_fg: Color::Blue,
            warning_fg: Color::Yellow,
            error_fg: Color::Red,
            muted_fg: Color::DarkGray,
            external_link_fg: Color::Yellow,
        }
    }

    /// Warm paper-like palette, the classic e-reader "sepia" mode.
    fn sepia_theme() -> Self {
        Self {
            text_fg: Some(Color::Rgb(91, 70, 54)),     // #5b4636  warm brown
            text_bg: Some(Color::Rgb(244, 236, 216)),  // #f4ecd8  paper
            highlight_fg: Color::Rgb(244, 236, 216),   // #f4ecd8
            highlight_bg: Color::Rgb(139, 111, 71),    // #8b6f47  tan
            annotation_highlight_fg: Color::Rgb(91, 70, 54),
            annotation_highlight_bg: Color::Rgb(232, 213, 163), // warm yellow
            annotation_green_bg: Color::Rgb(207, 217, 168),
            annotation_blue_bg: Color::Rgb(194, 212, 216),
            annotation_pink_bg: Color::Rgb(232, 196, 196),
            annotation_purple_bg: Color::Rgb(216, 200, 220),
            search_fg: Color::Rgb(91, 70, 54),
            search_bg: Color::Rgb(227, 197, 101), // muted gold
            search_current_fg: Color::Rgb(244, 236, 216),
            search_current_bg: Color::Rgb(191, 116, 46), // burnt orange
            info_fg: Color::Rgb(74, 108, 140),           // muted blue
            warning_fg: Color::Rgb(181, 137, 0),         // #b58900
            error_fg: Color::Rgb(157, 0, 6),             // #9d0006
            muted_fg: Color::Rgb(161, 145, 124),         // faded brown-gray
            external_link_fg: Color::Rgb(181, 137, 0),   // #b58900
        }
    }

    /// Gruvbox Dark — https://github.com/morhetz/gruvbox
    fn dark_theme() -> Self {
        Self {
            text_fg: Some(Color::Rgb(235, 219, 178)), // #ebdbb2  fg1
            text_bg: Some(Color::Rgb(40, 40, 40)),    // #282828  bg0
            highlight_fg: Color::Rgb(40, 40, 40),     // #282828  bg0
            highlight_bg: Color::Rgb(131, 165, 152),  // #83a598  bright-aqua
            annotation_highlight_fg: Color::Rgb(40, 40, 40), // #282828
            annotation_highlight_bg: Color::Rgb(235, 219, 178), // #ebdbb2  soft-yellow
            annotation_green_bg: Color::Rgb(152, 151, 26),   // #98971a  green
            annotation_blue_bg: Color::Rgb(69, 133, 136),    // #458588  blue
            annotation_pink_bg: Color::Rgb(211, 134, 155),   // #d3869b  bright-purple
            annotation_purple_bg: Color::Rgb(177, 98, 134),  // #b16286  purple
            search_fg: Color::Rgb(40, 40, 40),        // #282828
            search_bg: Color::Rgb(250, 189, 47),      // #fabd2f  bright-yellow
            search_current_fg: Color::Rgb(40, 40, 40), // #282828
            search_current_bg: Color::Rgb(254, 128, 25), // #fe8019  bright-orange
            info_fg: Color::Rgb(131, 165, 152),       // #83a598  bright-aqua
            warning_fg: Color::Rgb(250, 189, 47),     // #fabd2f  bright-yellow
            error_fg: Color::Rgb(251, 73, 52),        // #fb4934  bright-red
            muted_fg: Color::Rgb(146, 131, 116),      // #928374  gray
            external_link_fg: Color::Rgb(254, 128, 25), // #fe8019  bright-orange
        }
    }

    /// Gruvbox Light — https://github.com/morhetz/gruvbox
    fn light_theme() -> Self {
        Self {
            text_fg: Some(Color::Rgb(60, 56, 54)),           // #3c3836  fg1
            text_bg: Some(Color::Rgb(251, 241, 199)),        // #fbf1c7  bg0
            highlight_fg: Color::Rgb(251, 241, 199),         // #fbf1c7  bg0
            highlight_bg: Color::Rgb(7, 102, 120),           // #076678  dark-aqua
            annotation_highlight_fg: Color::Rgb(60, 56, 54), // #3c3836
            annotation_highlight_bg: Color::Rgb(242, 211, 135), // soft warm yellow
            annotation_green_bg: Color::Rgb(193, 214, 145),  // soft green
            annotation_blue_bg: Color::Rgb(168, 202, 214),   // soft blue
            annotation_pink_bg: Color::Rgb(238, 187, 195),   // soft pink
            annotation_purple_bg: Color::Rgb(212, 187, 220), // soft purple
            search_fg: Color::Rgb(251, 241, 199),            // #fbf1c7
            search_bg: Color::Rgb(181, 118, 20),             // #b57614  dark-yellow
            search_current_fg: Color::Rgb(251, 241, 199),    // #fbf1c7
            search_current_bg: Color::Rgb(175, 58, 3),       // #af3a03  dark-orange
            info_fg: Color::Rgb(7, 102, 120),                // #076678  dark-aqua
            warning_fg: Color::Rgb(181, 118, 20),            // #b57614  dark-yellow
            error_fg: Color::Rgb(157, 0, 6),                 // #9d0006  dark-red
            muted_fg: Color::Rgb(124, 111, 100),             // #7c6f64  gray
            external_link_fg: Color::Rgb(175, 58, 3),        // #af3a03  dark-orange
        }
    }
}
