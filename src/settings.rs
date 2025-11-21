use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum DoubleSpreadPadding {
    Left = 10,
    Middle = 7,
    Right = 10,
}

pub const VIEWER_PRESET_LIST: &[&str] = &[
    "feh",
    "imv",
    "gio",
    "gnome-open",
    "gvfs-open",
    "xdg-open",
    "kde-open",
    "firefox",
];

pub const DICT_PRESET_LIST: &[&str] = &[
    "wkdict",
    "sdcv",
    "dict",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub default_viewer: String,
    pub dictionary_client: String,
    pub show_progress_indicator: bool,
    pub page_scroll_animation: bool,
    pub mouse_support: bool,
    pub start_with_double_spread: bool,
    pub default_color_fg: i16,
    pub default_color_bg: i16,
    pub dark_color_fg: i16,
    pub dark_color_bg: i16,
    pub light_color_fg: i16,
    pub light_color_bg: i16,
    pub seamless_between_chapters: bool,
    pub preferred_tts_engine: Option<String>,
    pub tts_engine_args: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_viewer: "auto".to_string(),
            dictionary_client: "auto".to_string(),
            show_progress_indicator: true,
            page_scroll_animation: true,
            mouse_support: false,
            start_with_double_spread: false,
            default_color_fg: -1,
            default_color_bg: -1,
            dark_color_fg: 252,
            dark_color_bg: 235,
            light_color_fg: 238,
            light_color_bg: 253,
            seamless_between_chapters: false,
            preferred_tts_engine: None,
            tts_engine_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CfgDefaultKeymaps {
    pub scroll_up: String,
    pub scroll_down: String,
    pub page_up: String,
    pub page_down: String,
    pub next_chapter: String,
    pub prev_chapter: String,
    pub beginning_of_ch: String,
    pub end_of_ch: String,
    pub shrink: String,
    pub enlarge: String,
    pub set_width: String,
    pub metadata: String,
    pub define_word: String,
    pub table_of_contents: String,
    pub follow: String,
    pub open_image: String,
    pub regex_search: String,
    pub show_hide_progress: String,
    pub mark_position: String,
    pub jump_to_position: String,
    pub add_bookmark: String,
    pub show_bookmarks: String,
    pub quit: String,
    pub help: String,
    pub switch_color: String,
    pub tts_toggle: String,
    pub double_spread_toggle: String,
    pub library: String,
}

impl Default for CfgDefaultKeymaps {
    fn default() -> Self {
        Self {
            scroll_up: "k".to_string(),
            scroll_down: "j".to_string(),
            page_up: "h".to_string(),
            page_down: "l".to_string(),
            next_chapter: "L".to_string(),
            prev_chapter: "H".to_string(),
            beginning_of_ch: "g".to_string(),
            end_of_ch: "G".to_string(),
            shrink: "-".to_string(),
            enlarge: "+".to_string(),
            set_width: "=".to_string(),
            metadata: "M".to_string(),
            define_word: "d".to_string(),
            table_of_contents: "t".to_string(),
            follow: "f".to_string(),
            open_image: "o".to_string(),
            regex_search: "/".to_string(),
            show_hide_progress: "s".to_string(),
            mark_position: "m".to_string(),
            jump_to_position: "`".to_string(),
            add_bookmark: "b".to_string(),
            show_bookmarks: "B".to_string(),
            quit: "q".to_string(),
            help: "?".to_string(),
            switch_color: "c".to_string(),
            tts_toggle: "!".to_string(),
            double_spread_toggle: "D".to_string(),
            library: "R".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CfgBuiltinKeymaps {
    pub scroll_up: Vec<u16>,
    pub scroll_down: Vec<u16>,
    pub page_up: Vec<u16>,
    pub page_down: Vec<u16>,
    pub beginning_of_ch: Vec<u16>,
    pub end_of_ch: Vec<u16>,
    pub table_of_contents: Vec<u16>,
    pub follow: Vec<u16>,
    pub quit: Vec<u16>,
}

impl Default for CfgBuiltinKeymaps {
    fn default() -> Self {
        Self {
            scroll_up: vec![259], // curses.KEY_UP
            scroll_down: vec![258], // curses.KEY_DOWN
            page_up: vec![262, 260], // curses.KEY_PPAGE, curses.KEY_LEFT
            page_down: vec![263, ' '.into(), 261], // curses.KEY_NPAGE, ord(" "), curses.KEY_RIGHT
            beginning_of_ch: vec![268], // curses.KEY_HOME
            end_of_ch: vec![360], // curses.KEY_END
            table_of_contents: vec![9, '\t'.into()], // 9, ord("\t")
            follow: vec![10], // 10
            quit: vec![3, 27, 304], // 3, 27, 304
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keymap {
    pub add_bookmark: Vec<u16>,
    pub beginning_of_ch: Vec<u16>,
    pub define_word: Vec<u16>,
    pub double_spread_toggle: Vec<u16>,
    pub end_of_ch: Vec<u16>,
    pub enlarge: Vec<u16>,
    pub follow: Vec<u16>,
    pub help: Vec<u16>,
    pub jump_to_position: Vec<u16>,
    pub library: Vec<u16>,
    pub mark_position: Vec<u16>,
    pub metadata: Vec<u16>,
    pub next_chapter: Vec<u16>,
    pub open_image: Vec<u16>,
    pub page_down: Vec<u16>,
    pub page_up: Vec<u16>,
    pub prev_chapter: Vec<u16>,
    pub quit: Vec<u16>,
    pub regex_search: Vec<u16>,
    pub scroll_down: Vec<u16>,
    pub scroll_up: Vec<u16>,
    pub set_width: Vec<u16>,
    pub show_bookmarks: Vec<u16>,
    pub show_hide_progress: Vec<u16>,
    pub shrink: Vec<u16>,
    pub switch_color: Vec<u16>,
    pub tts_toggle: Vec<u16>,
    pub table_of_contents: Vec<u16>,
}