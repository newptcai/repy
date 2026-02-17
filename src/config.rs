use crate::settings::{CfgDefaultKeymaps, Keymap, Settings};
use eyre::Result;
use serde::Deserialize;
use serde_json;
use std::{fs, path::PathBuf};

/// Intermediate struct for deserializing the config file JSON.
#[derive(Deserialize)]
struct RawConfig {
    #[serde(default, rename = "Setting")]
    setting: Settings,
    #[serde(default, rename = "Keymap")]
    keymap_user: CfgDefaultKeymaps,
}

/// Parse settings and keymaps from a config file path.
/// Returns defaults for any missing or invalid fields.
fn parse_config_file(filepath: &PathBuf) -> Result<(Settings, CfgDefaultKeymaps)> {
    if filepath.exists() {
        let config_str = fs::read_to_string(filepath)?;
        if let Ok(raw) = serde_json::from_str::<RawConfig>(&config_str) {
            return Ok((raw.setting, raw.keymap_user));
        }
    }
    Ok((Settings::default(), CfgDefaultKeymaps::default()))
}

#[derive(Debug, Clone)]
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

        let (settings, keymap_user_dict) = if filepath.exists() {
            parse_config_file(&filepath)?
        } else {
            let settings = Settings::default();
            let keymap_user_dict = CfgDefaultKeymaps::default();
            let initial_config = serde_json::json!({
                "Setting": settings,
                "Keymap": keymap_user_dict,
            });
            fs::create_dir_all(&prefix)?;
            fs::write(&filepath, serde_json::to_string_pretty(&initial_config)?)?;
            (settings, keymap_user_dict)
        };

        let keymap = Keymap::default();

        Ok(Self {
            settings,
            keymap,
            keymap_user_dict,
            filepath,
        })
    }

    /// Get the configuration file path
    pub fn filepath(&self) -> &PathBuf {
        &self.filepath
    }

    /// Get the user-configured keymap dictionary (used for help menu text)
    pub fn keymap_user_dict(&self) -> &CfgDefaultKeymaps {
        &self.keymap_user_dict
    }

    /// Create a config with custom settings for testing
    pub fn with_settings(settings: Settings, keymap_user_dict: CfgDefaultKeymaps) -> Result<Self> {
        let prefix = get_app_data_prefix()?;
        let filepath = prefix.join("test_configuration.json");
        let keymap = Keymap::default();

        Ok(Self {
            settings,
            keymap,
            keymap_user_dict,
            filepath,
        })
    }

    /// Save current configuration to file
    pub fn save(&self) -> Result<()> {
        let config_json = serde_json::json!({
            "Setting": self.settings,
            "Keymap": self.keymap_user_dict,
        });

        let config_str = serde_json::to_string_pretty(&config_json)?;

        // Ensure directory exists before writing
        if let Some(parent) = self.filepath.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&self.filepath, config_str)?;
        Ok(())
    }

    /// Load configuration from a custom path
    pub fn load_from(filepath: PathBuf) -> Result<Self> {
        let (settings, keymap_user_dict) = parse_config_file(&filepath)?;
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

    Err(eyre::eyre!(
        "Could not determine application data directory"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{CfgDefaultKeymaps, Settings};
    use std::env;
    use std::sync::{Mutex, OnceLock};
    use tempfile::tempdir;

    fn lock_env() -> std::sync::MutexGuard<'static, ()> {
        static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("lock env mutex")
    }

    fn set_test_environment(dir: &tempfile::TempDir) {
        unsafe {
            env::set_var("XDG_CONFIG_HOME", dir.path());
            env::remove_var("HOME");
            env::remove_var("USERPROFILE");
        }
    }

    fn restore_test_environment(
        original_home: Option<std::ffi::OsString>,
        original_xdg_config_home: Option<std::ffi::OsString>,
        original_userprofile: Option<std::ffi::OsString>,
    ) {
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
        let _env_lock = lock_env();
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
        let loaded_keymaps: CfgDefaultKeymaps =
            serde_json::from_value(json_value["Keymap"].clone())?;
        assert_eq!(loaded_keymaps, default_keymaps);

        // Restore environment
        restore_test_environment(
            original_home,
            original_xdg_config_home,
            original_userprofile,
        );
        Ok(())
    }

    #[test]
    fn test_config_new_with_existing_file() -> Result<()> {
        let _env_lock = lock_env();
        let original_home = env::var_os("HOME");
        let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
        let original_userprofile = env::var_os("USERPROFILE");

        let dir = tempdir()?;
        set_test_environment(&dir);

        // Create a config file with custom settings
        let config_path = dir.path().join("repy").join("configuration.json");
        std::fs::create_dir_all(config_path.parent().unwrap())?;

        let config_json = serde_json::json!({
            "Setting": {
                "mouse_support": true,
                "default_viewer": "custom_viewer"
            },
            "Keymap": {
                "quit": "Q",
                "help": "H"
            }
        });

        std::fs::write(&config_path, serde_json::to_string(&config_json)?)?;

        // Test that the config loads correctly
        let config = Config::new()?;
        assert_eq!(config.settings.mouse_support, true);
        assert_eq!(config.settings.default_viewer, "custom_viewer");
        assert_eq!(config.keymap_user_dict().quit, "Q");
        assert_eq!(config.keymap_user_dict().help, "H");

        // Restore environment
        restore_test_environment(
            original_home,
            original_xdg_config_home,
            original_userprofile,
        );
        Ok(())
    }

    #[test]
    fn test_get_app_data_prefix() {
        let _env_lock = lock_env();
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

            // Test USERPROFILE (Windows)
            let profile_dir = tempdir().unwrap();
            let profile_repy_dir = profile_dir.path().join(".repy");
            std::fs::create_dir_all(&profile_repy_dir).unwrap();
            env::set_var("USERPROFILE", profile_dir.path());
            env::remove_var("HOME");
            env::remove_var("XDG_CONFIG_HOME");
            assert_eq!(get_app_data_prefix().unwrap(), profile_repy_dir);

            // Test error case - no environment variables set
            env::remove_var("HOME");
            env::remove_var("XDG_CONFIG_HOME");
            env::remove_var("USERPROFILE");
            assert!(get_app_data_prefix().is_err());

            // Restore original environment variables using the helper function
            restore_test_environment(
                original_home,
                original_xdg_config_home,
                original_userprofile,
            );
        }
    }

    #[test]
    fn test_config_accessors() -> Result<()> {
        let _env_lock = lock_env();
        let original_home = env::var_os("HOME");
        let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
        let original_userprofile = env::var_os("USERPROFILE");

        let dir = tempdir()?;
        set_test_environment(&dir);

        let config = Config::new()?;

        // Test accessors
        assert_eq!(
            config.filepath(),
            &dir.path().join("repy").join("configuration.json")
        );
        assert_eq!(config.keymap_user_dict().scroll_up, "k");
        assert_eq!(config.keymap_user_dict().scroll_down, "j");

        // Restore environment
        restore_test_environment(
            original_home,
            original_xdg_config_home,
            original_userprofile,
        );
        Ok(())
    }

    #[test]
    fn test_config_with_custom_settings() -> Result<()> {
        let _env_lock = lock_env();
        let original_home = env::var_os("HOME");
        let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
        let original_userprofile = env::var_os("USERPROFILE");

        let dir = tempdir()?;
        set_test_environment(&dir);

        let mut custom_settings = Settings::default();
        custom_settings.mouse_support = true;
        custom_settings.default_viewer = "feh".to_string();

        let mut custom_keymaps = CfgDefaultKeymaps::default();
        custom_keymaps.scroll_up = "K".to_string();
        custom_keymaps.quit = "Q".to_string();

        let config = Config::with_settings(custom_settings.clone(), custom_keymaps.clone())?;

        assert_eq!(config.settings.mouse_support, true);
        assert_eq!(config.settings.default_viewer, "feh");
        assert_eq!(config.keymap_user_dict().scroll_up, "K");
        assert_eq!(config.keymap_user_dict().quit, "Q");

        // Restore environment
        restore_test_environment(
            original_home,
            original_xdg_config_home,
            original_userprofile,
        );
        Ok(())
    }

    #[test]
    fn test_config_save_and_load() -> Result<()> {
        let _env_lock = lock_env();
        let original_home = env::var_os("HOME");
        let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
        let original_userprofile = env::var_os("USERPROFILE");

        let dir = tempdir()?;
        set_test_environment(&dir);

        let mut custom_settings = Settings::default();
        custom_settings.page_scroll_animation = false;
        custom_settings.dark_color_fg = 255;

        let mut custom_keymaps = CfgDefaultKeymaps::default();
        custom_keymaps.help = "H".to_string();
        custom_keymaps.metadata = "I".to_string();

        let config = Config::with_settings(custom_settings.clone(), custom_keymaps.clone())?;
        config.save()?;

        let saved_path = config.filepath();
        assert!(saved_path.exists());

        // Load the saved config
        let loaded_config = Config::load_from(saved_path.clone())?;
        assert_eq!(loaded_config.settings.page_scroll_animation, false);
        assert_eq!(loaded_config.settings.dark_color_fg, 255);

        // Clean up
        std::fs::remove_file(saved_path)?;

        // Restore environment
        restore_test_environment(
            original_home,
            original_xdg_config_home,
            original_userprofile,
        );
        Ok(())
    }

    #[test]
    fn test_config_invalid_json() -> Result<()> {
        let _env_lock = lock_env();
        let original_home = env::var_os("HOME");
        let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
        let original_userprofile = env::var_os("USERPROFILE");

        let dir = tempdir()?;
        set_test_environment(&dir);

        let config_path = dir.path().join("repy").join("invalid_config.json");
        std::fs::create_dir_all(config_path.parent().unwrap())?;

        // Write invalid JSON
        std::fs::write(&config_path, "{ invalid json }")?;

        // Loading should fallback to defaults
        let config = Config::load_from(config_path.clone())?;
        let default_settings = Settings::default();
        assert_eq!(config.settings, default_settings);

        // Clean up
        std::fs::remove_file(&config_path)?;

        // Restore environment
        restore_test_environment(
            original_home,
            original_xdg_config_home,
            original_userprofile,
        );
        Ok(())
    }

    #[test]
    fn test_config_partial_settings() -> Result<()> {
        let _env_lock = lock_env();
        let original_home = env::var_os("HOME");
        let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
        let original_userprofile = env::var_os("USERPROFILE");

        let dir = tempdir()?;
        set_test_environment(&dir);

        let config_path = dir.path().join("repy").join("partial_config.json");
        std::fs::create_dir_all(config_path.parent().unwrap())?;

        // Write config with only some settings
        let partial_config = serde_json::json!({
            "Setting": {
                "mouse_support": true,
                "default_color_fg": 100
            },
            "Keymap": {
                "scroll_up": "K",
                "quit": "Q"
            }
        });

        std::fs::write(&config_path, serde_json::to_string(&partial_config)?)?;

        let config = Config::load_from(config_path.clone())?;

        // Custom settings should be loaded
        assert_eq!(config.settings.mouse_support, true);
        assert_eq!(config.settings.default_color_fg, 100);
        assert_eq!(config.keymap_user_dict().scroll_up, "K");
        assert_eq!(config.keymap_user_dict().quit, "Q");

        // Default settings should remain for unspecified values
        assert_eq!(config.settings.default_viewer, "auto");
        assert_eq!(config.settings.dark_color_fg, 252);

        // Clean up
        std::fs::remove_file(&config_path)?;

        // Restore environment
        restore_test_environment(
            original_home,
            original_xdg_config_home,
            original_userprofile,
        );
        Ok(())
    }

    #[test]
    fn test_config_edge_cases() -> Result<()> {
        let _env_lock = lock_env();
        let original_home = env::var_os("HOME");
        let original_xdg_config_home = env::var_os("XDG_CONFIG_HOME");
        let original_userprofile = env::var_os("USERPROFILE");

        let dir = tempdir()?;
        set_test_environment(&dir);

        // Test empty config file
        let empty_config_path = dir.path().join("repy").join("empty_config.json");
        std::fs::create_dir_all(empty_config_path.parent().unwrap())?;
        std::fs::write(&empty_config_path, "")?;

        let config = Config::load_from(empty_config_path.clone())?;
        let default_settings = Settings::default();
        assert_eq!(config.settings, default_settings);

        // Test config with only Setting section
        let settings_only_path = dir.path().join("repy").join("settings_only.json");
        let settings_only = serde_json::json!({
            "Setting": {
                "show_progress_indicator": false,
                "page_scroll_animation": false
            }
        });
        std::fs::write(&settings_only_path, serde_json::to_string(&settings_only)?)?;

        let config2 = Config::load_from(settings_only_path.clone())?;
        assert!(!config2.settings.show_progress_indicator);
        assert!(!config2.settings.page_scroll_animation);
        assert_eq!(config2.keymap_user_dict().scroll_up, "k"); // Should be default

        // Test config with only Keymap section
        let keymap_only_path = dir.path().join("repy").join("keymap_only.json");
        let keymap_only = serde_json::json!({
            "Keymap": {
                "quit": "Q",
                "help": "H"
            }
        });
        std::fs::write(&keymap_only_path, serde_json::to_string(&keymap_only)?)?;

        let config3 = Config::load_from(keymap_only_path.clone())?;
        assert_eq!(config3.keymap_user_dict().quit, "Q");
        assert_eq!(config3.keymap_user_dict().help, "H");
        assert_eq!(config3.settings.mouse_support, false); // Should be default

        // Clean up
        std::fs::remove_file(&empty_config_path)?;
        std::fs::remove_file(&settings_only_path)?;
        std::fs::remove_file(&keymap_only_path)?;

        // Restore environment
        restore_test_environment(
            original_home,
            original_xdg_config_home,
            original_userprofile,
        );
        Ok(())
    }
}
