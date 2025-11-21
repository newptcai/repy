use crate::settings::{Settings, Keymap, CfgDefaultKeymaps, CfgBuiltinKeymaps};
use eyre::Result;
use std::{fs, path::PathBuf};
use serde_json; // Needed for serde_json::from_value and serde_json::json!

pub struct Config {
    pub settings: Settings,
    pub keymap: Keymap,
    keymap_user_dict: CfgDefaultKeymaps, // Used for building help menu text, will be private
    filepath: PathBuf,
}

impl Config {
    pub fn new() -> Result<Self> {
        let prefix = get_app_data_prefix()?;
        let filepath = prefix.join("configuration.json");

        let default_settings = Settings::default();
        let default_keymaps_str = CfgDefaultKeymaps::default();
        let builtin_keymaps_u16 = CfgBuiltinKeymaps::default();

        let mut settings = default_settings;
        let mut keymap_user_dict = default_keymaps_str;

        if filepath.exists() {
            let config_str = fs::read_to_string(&filepath)?;
            let user_config: serde_json::Value = serde_json::from_str(&config_str)?;

            // Merge settings
            if let Some(user_settings_value) = user_config.get("Setting") {
                // Deserialize to temp settings to merge
                let temp_settings: Settings = serde_json::from_value(user_settings_value.clone())?;
                // This is a basic merge, a more robust merge would iterate and update fields
                // For now, this overwrites if present in user_settings_value
                settings = temp_settings;
            }

            // Merge keymaps
            if let Some(user_keymap_value) = user_config.get("Keymap") {
                // Deserialize to temp keymaps to merge
                let temp_keymaps: CfgDefaultKeymaps = serde_json::from_value(user_keymap_value.clone())?;
                keymap_user_dict = temp_keymaps;
            }
        } else {
            // Save initial config if it doesn't exist
            let initial_config = serde_json::json!({
                "Setting": settings,
                "Keymap": keymap_user_dict,
            });
            fs::create_dir_all(&prefix)?;
            fs::write(&filepath, serde_json::to_string_pretty(&initial_config)?)?;
        }

        // Construct the final Keymap by merging user, default, and builtin keymaps
        // This will involve parsing the string keycodes into u16, and then combining with builtin_keymaps_u16
        // For now, a placeholder, will implement the parsing and merging logic later.
        let keymap = Keymap {
            // Placeholder: will implement proper merging later
            add_bookmark: Vec::new(),
            beginning_of_ch: Vec::new(),
            define_word: Vec::new(),
            double_spread_toggle: Vec::new(),
            end_of_ch: Vec::new(),
            enlarge: Vec::new(),
            follow: Vec::new(),
            help: Vec::new(),
            jump_to_position: Vec::new(),
            library: Vec::new(),
            mark_position: Vec::new(),
            metadata: Vec::new(),
            next_chapter: Vec::new(),
            open_image: Vec::new(),
            page_down: Vec::new(),
            page_up: Vec::new(),
            prev_chapter: Vec::new(),
            quit: Vec::new(),
            regex_search: Vec::new(),
            scroll_down: Vec::new(),
            scroll_up: Vec::new(),
            set_width: Vec::new(),
            show_bookmarks: Vec::new(),
            show_hide_progress: Vec::new(),
            shrink: Vec::new(),
            switch_color: Vec::new(),
            tts_toggle: Vec::new(),
            table_of_contents: Vec::new(),
        };


        Ok(Self {
            settings,
            keymap,
            keymap_user_dict,
            filepath,
        })
    }
}

pub fn get_app_data_prefix() -> Result<PathBuf> {
    // This function corresponds to AppData's prefix logic in Python
    // Will implement this logic properly. For now, a placeholder.
    // It should check XDG_CONFIG_HOME first, then HOME/.config, then HOME.
    // On Windows, it would be USERPROFILE.

    if let Some(config_home) = std::env::var_os("XDG_CONFIG_HOME") {
        let path = PathBuf::from(config_home).join("repy");
        return Ok(path);
    } else if let Some(home) = std::env::var_os("HOME") {
        let path = PathBuf::from(home.clone()).join(".config").join("repy");
        if path.exists() {
            return Ok(path);
        } else {
            return Ok(PathBuf::from(home).join(".repy"));
        }
    } else if let Some(user_profile) = std::env::var_os("USERPROFILE") {
        return Ok(PathBuf::from(user_profile).join(".repy"));
    }

    // Fallback if no known home directory is found
    // This should probably be an error or a temporary directory
    Err(eyre::eyre!("Could not determine application data directory"))
}