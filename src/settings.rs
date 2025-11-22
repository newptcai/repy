use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Copy, Serialize, Deserialize)]
pub struct DoubleSpreadPadding {
    pub left: u16,
    pub middle: u16,
    pub right: u16,
}

impl Default for DoubleSpreadPadding {
    fn default() -> Self {
        Self {
            left: 10,
            middle: 7,
            right: 10,
        }
    }
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
#[serde(default)]
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

impl Settings {
    pub fn merge(&mut self, other: Self) {
        self.default_viewer = other.default_viewer;
        self.dictionary_client = other.dictionary_client;
        self.show_progress_indicator = other.show_progress_indicator;
        self.page_scroll_animation = other.page_scroll_animation;
        self.mouse_support = other.mouse_support;
        self.start_with_double_spread = other.start_with_double_spread;
        self.default_color_fg = other.default_color_fg;
        self.default_color_bg = other.default_color_bg;
        self.dark_color_fg = other.dark_color_fg;
        self.dark_color_bg = other.dark_color_bg;
        self.light_color_fg = other.light_color_fg;
        self.light_color_bg = other.light_color_bg;
        self.seamless_between_chapters = other.seamless_between_chapters;
        if other.preferred_tts_engine.is_some() {
            self.preferred_tts_engine = other.preferred_tts_engine;
        }
        if !other.tts_engine_args.is_empty() {
            self.tts_engine_args = other.tts_engine_args;
        }
    }
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
#[serde(default)]
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

impl CfgDefaultKeymaps {
    pub fn merge(&mut self, other: Self) {
        self.scroll_up = other.scroll_up;
        self.scroll_down = other.scroll_down;
        self.page_up = other.page_up;
        self.page_down = other.page_down;
        self.next_chapter = other.next_chapter;
        self.prev_chapter = other.prev_chapter;
        self.beginning_of_ch = other.beginning_of_ch;
        self.end_of_ch = other.end_of_ch;
        self.shrink = other.shrink;
        self.enlarge = other.enlarge;
        self.set_width = other.set_width;
        self.metadata = other.metadata;
        self.define_word = other.define_word;
        self.table_of_contents = other.table_of_contents;
        self.follow = other.follow;
        self.open_image = other.open_image;
        self.regex_search = other.regex_search;
        self.show_hide_progress = other.show_hide_progress;
        self.mark_position = other.mark_position;
        self.jump_to_position = other.jump_to_position;
        self.add_bookmark = other.add_bookmark;
        self.show_bookmarks = other.show_bookmarks;
        self.quit = other.quit;
        self.help = other.help;
        self.switch_color = other.switch_color;
        self.tts_toggle = other.tts_toggle;
        self.double_spread_toggle = other.double_spread_toggle;
        self.library = other.library;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
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
            page_down: vec![263, ' ' as u16, 261], // curses.KEY_NPAGE, ord(" "), curses.KEY_RIGHT
            beginning_of_ch: vec![268], // curses.KEY_HOME
            end_of_ch: vec![360], // curses.KEY_END
            table_of_contents: vec![9, '\t' as u16], // 9, ord("\t")
            follow: vec![10], // 10
            quit: vec![3, 27, 304], // 3, 27, 304
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_spread_padding_default() {
        let padding = DoubleSpreadPadding::default();
        assert_eq!(padding.left, 10);
        assert_eq!(padding.middle, 7);
        assert_eq!(padding.right, 10);
    }

    #[test]
    fn test_settings_default() {
        let settings = Settings::default();
        assert_eq!(settings.default_viewer, "auto");
        assert_eq!(settings.dictionary_client, "auto");
        assert!(settings.show_progress_indicator);
        assert!(settings.page_scroll_animation);
        assert!(!settings.mouse_support);
        assert!(!settings.start_with_double_spread);
        assert_eq!(settings.default_color_fg, -1);
        assert_eq!(settings.default_color_bg, -1);
        assert_eq!(settings.dark_color_fg, 252);
        assert_eq!(settings.dark_color_bg, 235);
        assert_eq!(settings.light_color_fg, 238);
        assert_eq!(settings.light_color_bg, 253);
        assert!(!settings.seamless_between_chapters);
        assert_eq!(settings.preferred_tts_engine, None);
        assert!(settings.tts_engine_args.is_empty());
    }

    #[test]
    fn test_cfg_default_keymaps_default() {
        let keymaps = CfgDefaultKeymaps::default();
        assert_eq!(keymaps.scroll_up, "k");
        assert_eq!(keymaps.scroll_down, "j");
        assert_eq!(keymaps.page_up, "h");
        assert_eq!(keymaps.page_down, "l");
        assert_eq!(keymaps.next_chapter, "L");
        assert_eq!(keymaps.prev_chapter, "H");
        assert_eq!(keymaps.beginning_of_ch, "g");
        assert_eq!(keymaps.end_of_ch, "G");
        assert_eq!(keymaps.shrink, "-");
        assert_eq!(keymaps.enlarge, "+");
        assert_eq!(keymaps.set_width, "=");
        assert_eq!(keymaps.metadata, "M");
        assert_eq!(keymaps.define_word, "d");
        assert_eq!(keymaps.table_of_contents, "t");
        assert_eq!(keymaps.follow, "f");
        assert_eq!(keymaps.open_image, "o");
        assert_eq!(keymaps.regex_search, "/");
        assert_eq!(keymaps.show_hide_progress, "s");
        assert_eq!(keymaps.mark_position, "m");
        assert_eq!(keymaps.jump_to_position, "`");
        assert_eq!(keymaps.add_bookmark, "b");
        assert_eq!(keymaps.show_bookmarks, "B");
        assert_eq!(keymaps.quit, "q");
        assert_eq!(keymaps.help, "?");
        assert_eq!(keymaps.switch_color, "c");
        assert_eq!(keymaps.tts_toggle, "!");
        assert_eq!(keymaps.double_spread_toggle, "D");
        assert_eq!(keymaps.library, "R");
    }

    #[test]
    fn test_cfg_builtin_keymaps_default() {
        let keymaps = CfgBuiltinKeymaps::default();
        assert_eq!(keymaps.scroll_up, vec![259]);
        assert_eq!(keymaps.scroll_down, vec![258]);
        assert_eq!(keymaps.page_up, vec![262, 260]);
        assert_eq!(keymaps.page_down, vec![263, ' ' as u16, 261]);
        assert_eq!(keymaps.beginning_of_ch, vec![268]);
        assert_eq!(keymaps.end_of_ch, vec![360]);
        assert_eq!(keymaps.table_of_contents, vec![9, '\t' as u16]);
        assert_eq!(keymaps.follow, vec![10]);
        assert_eq!(keymaps.quit, vec![3, 27, 304]);
    }
}