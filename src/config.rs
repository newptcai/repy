use crate::settings::{Settings, Keymap, CfgDefaultKeymaps};
use eyre::Result;
use std::{fs, path::PathBuf};
use serde_json;

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

        let mut settings = Settings::default();
        let mut keymap_user_dict = CfgDefaultKeymaps::default();

        if filepath.exists() {
            let config_str = fs::read_to_string(&filepath)?;
            if let Ok(user_config) = serde_json::from_str::<serde_json::Value>(&config_str) {
                if let Some(user_settings_value) = user_config.get("Setting") {
                    if let serde_json::Value::Object(user_settings_map) = user_settings_value {
                        if let Some(val) = user_settings_map.get("default_viewer").and_then(|v| v.as_str()) { settings.default_viewer = val.to_string(); }
                        if let Some(val) = user_settings_map.get("dictionary_client").and_then(|v| v.as_str()) { settings.dictionary_client = val.to_string(); }
                        if let Some(val) = user_settings_map.get("show_progress_indicator").and_then(|v| v.as_bool()) { settings.show_progress_indicator = val; }
                        if let Some(val) = user_settings_map.get("page_scroll_animation").and_then(|v| v.as_bool()) { settings.page_scroll_animation = val; }
                        if let Some(val) = user_settings_map.get("mouse_support").and_then(|v| v.as_bool()) { settings.mouse_support = val; }
                        if let Some(val) = user_settings_map.get("start_with_double_spread").and_then(|v| v.as_bool()) { settings.start_with_double_spread = val; }
                        if let Some(val) = user_settings_map.get("default_color_fg").and_then(|v| v.as_i64()) { settings.default_color_fg = val as i16; }
                        if let Some(val) = user_settings_map.get("default_color_bg").and_then(|v| v.as_i64()) { settings.default_color_bg = val as i16; }
                        if let Some(val) = user_settings_map.get("dark_color_fg").and_then(|v| v.as_i64()) { settings.dark_color_fg = val as i16; }
                        if let Some(val) = user_settings_map.get("dark_color_bg").and_then(|v| v.as_i64()) { settings.dark_color_bg = val as i16; }
                        if let Some(val) = user_settings_map.get("light_color_fg").and_then(|v| v.as_i64()) { settings.light_color_fg = val as i16; }
                        if let Some(val) = user_settings_map.get("light_color_bg").and_then(|v| v.as_i64()) { settings.light_color_bg = val as i16; }
                        if let Some(val) = user_settings_map.get("seamless_between_chapters").and_then(|v| v.as_bool()) { settings.seamless_between_chapters = val; }
                        if let Some(val) = user_settings_map.get("preferred_tts_engine").and_then(|v| v.as_str()) { settings.preferred_tts_engine = Some(val.to_string()); }
                        if let Some(val) = user_settings_map.get("tts_engine_args").and_then(|v| v.as_array()) {
                            settings.tts_engine_args = val.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect();
                        }
                    }
                }

                if let Some(user_keymap_value) = user_config.get("Keymap") {
                    if let serde_json::Value::Object(user_keymap_map) = user_keymap_value {
                        if let Some(val) = user_keymap_map.get("scroll_up").and_then(|v| v.as_str()) { keymap_user_dict.scroll_up = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("scroll_down").and_then(|v| v.as_str()) { keymap_user_dict.scroll_down = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("page_up").and_then(|v| v.as_str()) { keymap_user_dict.page_up = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("page_down").and_then(|v| v.as_str()) { keymap_user_dict.page_down = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("next_chapter").and_then(|v| v.as_str()) { keymap_user_dict.next_chapter = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("prev_chapter").and_then(|v| v.as_str()) { keymap_user_dict.prev_chapter = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("beginning_of_ch").and_then(|v| v.as_str()) { keymap_user_dict.beginning_of_ch = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("end_of_ch").and_then(|v| v.as_str()) { keymap_user_dict.end_of_ch = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("shrink").and_then(|v| v.as_str()) { keymap_user_dict.shrink = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("enlarge").and_then(|v| v.as_str()) { keymap_user_dict.enlarge = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("set_width").and_then(|v| v.as_str()) { keymap_user_dict.set_width = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("metadata").and_then(|v| v.as_str()) { keymap_user_dict.metadata = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("define_word").and_then(|v| v.as_str()) { keymap_user_dict.define_word = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("table_of_contents").and_then(|v| v.as_str()) { keymap_user_dict.table_of_contents = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("follow").and_then(|v| v.as_str()) { keymap_user_dict.follow = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("open_image").and_then(|v| v.as_str()) { keymap_user_dict.open_image = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("regex_search").and_then(|v| v.as_str()) { keymap_user_dict.regex_search = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("show_hide_progress").and_then(|v| v.as_str()) { keymap_user_dict.show_hide_progress = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("mark_position").and_then(|v| v.as_str()) { keymap_user_dict.mark_position = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("jump_to_position").and_then(|v| v.as_str()) { keymap_user_dict.jump_to_position = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("add_bookmark").and_then(|v| v.as_str()) { keymap_user_dict.add_bookmark = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("show_bookmarks").and_then(|v| v.as_str()) { keymap_user_dict.show_bookmarks = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("quit").and_then(|v| v.as_str()) { keymap_user_dict.quit = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("help").and_then(|v| v.as_str()) { keymap_user_dict.help = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("switch_color").and_then(|v| v.as_str()) { keymap_user_dict.switch_color = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("tts_toggle").and_then(|v| v.as_str()) { keymap_user_dict.tts_toggle = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("double_spread_toggle").and_then(|v| v.as_str()) { keymap_user_dict.double_spread_toggle = val.to_string(); }
                        if let Some(val) = user_keymap_map.get("library").and_then(|v| v.as_str()) { keymap_user_dict.library = val.to_string(); }
                    }
                }
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

        let keymap = Keymap::default();


        Ok(Self {
            settings,
            keymap,
            keymap_user_dict,
            filepath,
        })
    }
}

pub fn get_app_data_prefix() -> Result<PathBuf> {
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

    Err(eyre::eyre!("Could not determine application data directory"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::tempdir;
    use crate::settings::{Settings, CfgDefaultKeymaps};

    fn set_test_environment(dir: &tempfile::TempDir) {
        unsafe {
            env::set_var("XDG_CONFIG_HOME", dir.path());
            env::remove_var("HOME");
            env::remove_var("USERPROFILE");
        }
    }

    fn restore_test_environment(original_home: Option<std::ffi::OsString>,
                              original_xdg_config_home: Option<std::ffi::OsString>,
                              original_userprofile: Option<std::ffi::OsString>) {
        unsafe {
            if let Some(home) = original_home {
                env::set_var("HOME", home);
            } else {
                env::remove_var("HOME");
            }
            if let Some(xdg) = original_xdg_config_home {
                env::set_var("XDG_CONFIG_HOME", xdg);
            } else {
                env::remove_var("XDG_CONFIG_HOME");
            }
            if let Some(profile) = original_userprofile {
                env::set_var("USERPROFILE", profile);
            } else {
                env::remove_var("USERPROFILE");
            }
        }
    }

    #[test]
    fn test_config_new_no_existing_file() -> Result<()> {
        let original_home = env::var_os("HOME");
        let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
        let original_userprofile = env::var_os("USERPROFILE");

        let dir = tempdir()?;
        set_test_environment(&dir);

        let config = Config::new()?;
        let expected_filepath = dir.path().join("repy").join("configuration.json");

        assert_eq!(config.filepath, expected_filepath);
        assert!(expected_filepath.exists());

        let config_str = fs::read_to_string(&expected_filepath)?;
        let json_value: serde_json::Value = serde_json::from_str(&config_str)?;

        let default_settings = Settings::default();
        let loaded_settings: Settings = serde_json::from_value(json_value["Setting"].clone())?;
        assert_eq!(loaded_settings, default_settings);

        let default_keymaps = CfgDefaultKeymaps::default();
        let loaded_keymaps: CfgDefaultKeymaps = serde_json::from_value(json_value["Keymap"].clone())?;
        assert_eq!(loaded_keymaps, default_keymaps);

        // Restore environment
        restore_test_environment(original_home, original_xdg_config_home, original_userprofile);
        Ok(())
    }

    #[test]
    fn test_config_new_with_existing_file() -> Result<()> {
        // Test the config loading functionality by creating a config file
        // in the system's actual config directory and testing that it loads correctly
        // This avoids environment variable pollution issues in parallel tests

        // Get the actual config directory that Config::new() will use
        let config_dir = get_app_data_prefix()?;
        let config_file_path = config_dir.join("configuration.json");

        // Save original config file if it exists
        let original_config_exists = config_file_path.exists();
        let original_config_content = if original_config_exists {
            Some(std::fs::read_to_string(&config_file_path)?)
        } else {
            None
        };

        // Create our test config
        std::fs::create_dir_all(&config_dir)?;
        let mut settings_map = serde_json::Map::new();
        settings_map.insert("mouse_support".to_string(), serde_json::Value::Bool(true));
        let custom_settings = serde_json::Value::Object(settings_map);

        let config_json = serde_json::json!({
            "Setting": custom_settings,
            "Keymap": CfgDefaultKeymaps::default(),
        });
        std::fs::write(&config_file_path, serde_json::to_string(&config_json)?)?;

        // Test that the config loads correctly
        let config = Config::new()?;
        assert_eq!(config.settings.mouse_support, true);

        // Restore original config
        if let Some(original_content) = original_config_content {
            std::fs::write(&config_file_path, original_content)?;
        } else if original_config_exists {
            std::fs::remove_file(&config_file_path)?;
        }

        Ok(())
    }

    #[test]
    fn test_get_app_data_prefix() {
        let original_home = env::var_os("HOME");
        let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
        let original_userprofile = env::var_os("USERPROFILE");

        unsafe {
            // Test XDG_CONFIG_HOME
            let xdg_dir = tempdir().unwrap();
            env::set_var("XDG_CONFIG_HOME", xdg_dir.path());
            env::remove_var("HOME");
            env::remove_var("USERPROFILE");
            let expected_path = xdg_dir.path().join("repy");
            assert_eq!(get_app_data_prefix().unwrap(), expected_path);

            // Test HOME/.config
            let home_dir = tempdir().unwrap();
            let config_dir = home_dir.path().join(".config").join("repy");
            std::fs::create_dir_all(&config_dir).unwrap();
            env::set_var("HOME", home_dir.path());
            env::remove_var("XDG_CONFIG_HOME");
            assert_eq!(get_app_data_prefix().unwrap(), config_dir);

            // Test HOME/.repy (legacy)
            let home_dir_legacy = tempdir().unwrap();
            let repy_dir = home_dir_legacy.path().join(".repy");
            std::fs::create_dir_all(&repy_dir).unwrap();
            let config_dir_legacy = home_dir_legacy.path().join(".config");
            if config_dir_legacy.exists() {
                std::fs::remove_dir_all(&config_dir_legacy).unwrap();
            }
            env::set_var("HOME", home_dir_legacy.path());
            env::remove_var("XDG_CONFIG_HOME");
            assert_eq!(get_app_data_prefix().unwrap(), repy_dir);

            // Restore original environment variables using the helper function
            restore_test_environment(original_home, original_xdg_config_home, original_userprofile);
        }
    }
}
