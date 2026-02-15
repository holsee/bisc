//! User settings persistence via TOML.
//!
//! Settings are stored at `<config_dir>/bisc/settings.toml`.
//! Missing or corrupted config files return sensible defaults.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// User-configurable settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Display name shown to other peers.
    pub display_name: String,
    /// Directory for file storage.
    pub storage_dir: PathBuf,
    /// Video quality preset.
    pub video_quality: Quality,
    /// Audio quality preset.
    pub audio_quality: Quality,
    /// Preferred audio input device name (empty = system default).
    pub input_device: String,
    /// Preferred audio output device name (empty = system default).
    pub output_device: String,
    /// UI theme.
    pub theme: Theme,
}

/// Quality level preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Low,
    Medium,
    High,
}

/// UI theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

/// Resolved directory paths for the application instance.
#[derive(Debug, Clone)]
pub struct AppDirs {
    /// Config directory (settings.toml lives here).
    pub config_dir: PathBuf,
    /// Default storage directory (SQLite DB, downloaded files).
    pub default_storage_dir: PathBuf,
}

impl AppDirs {
    /// Resolve directories from an optional `--data-dir` override.
    ///
    /// With override: config → `<data_dir>/config/`, storage → `<data_dir>/data/`
    /// Without: platform defaults via the `directories` crate.
    pub fn resolve(data_dir_override: Option<&PathBuf>) -> Self {
        match data_dir_override {
            Some(data_dir) => Self {
                config_dir: data_dir.join("config"),
                default_storage_dir: data_dir.join("data"),
            },
            None => {
                let project_dirs = directories::ProjectDirs::from("", "", "bisc");
                Self {
                    config_dir: project_dirs
                        .as_ref()
                        .map(|d| d.config_dir().to_path_buf())
                        .unwrap_or_else(|| PathBuf::from("bisc-config")),
                    default_storage_dir: project_dirs
                        .as_ref()
                        .map(|d| d.data_dir().to_path_buf())
                        .unwrap_or_else(|| PathBuf::from("bisc-data")),
                }
            }
        }
    }
}

impl Settings {
    /// Create default settings using the given app directories.
    pub fn default_with_dirs(dirs: &AppDirs) -> Self {
        Self {
            display_name: "user".to_string(),
            storage_dir: dirs.default_storage_dir.clone(),
            video_quality: Quality::Medium,
            audio_quality: Quality::Medium,
            input_device: String::new(),
            output_device: String::new(),
            theme: Theme::Dark,
        }
    }

    /// Load settings from the given app directories.
    ///
    /// Returns defaults if the file doesn't exist or is corrupted.
    pub fn load(dirs: &AppDirs) -> Self {
        Self::load_from_dir(dirs.config_dir.clone(), dirs)
    }

    /// Save settings to the given app directories.
    pub fn save(&self, dirs: &AppDirs) -> Result<()> {
        self.save_to_dir(dirs.config_dir.clone())
    }

    /// Load settings from a specific config directory.
    pub fn load_from_dir(config_dir: PathBuf, dirs: &AppDirs) -> Self {
        let path = config_dir.join("settings.toml");
        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str(&contents) {
                Ok(settings) => {
                    tracing::info!(path = %path.display(), "settings loaded");
                    settings
                }
                Err(e) => {
                    tracing::warn!(
                        path = %path.display(),
                        error = %e,
                        "corrupted settings file, using defaults"
                    );
                    Self::default_with_dirs(dirs)
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    path = %path.display(),
                    "settings file not found, using defaults"
                );
                Self::default_with_dirs(dirs)
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to read settings file, using defaults"
                );
                Self::default_with_dirs(dirs)
            }
        }
    }

    /// Save settings to a specific config directory.
    pub fn save_to_dir(&self, config_dir: PathBuf) -> Result<()> {
        std::fs::create_dir_all(&config_dir).with_context(|| {
            format!(
                "failed to create config directory: {}",
                config_dir.display()
            )
        })?;

        let path = config_dir.join("settings.toml");
        let contents = toml::to_string_pretty(self).context("failed to serialize settings")?;
        std::fs::write(&path, &contents)
            .with_context(|| format!("failed to write settings file: {}", path.display()))?;

        tracing::info!(path = %path.display(), "settings saved");
        Ok(())
    }
}

impl Default for Settings {
    fn default() -> Self {
        let dirs = AppDirs::resolve(None);
        Self::default_with_dirs(&dirs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_test_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "debug".into()),
            )
            .with_test_writer()
            .try_init();
    }

    fn test_dirs(config_dir: PathBuf) -> AppDirs {
        AppDirs {
            config_dir: config_dir.clone(),
            default_storage_dir: config_dir.join("data"),
        }
    }

    #[test]
    fn default_settings_are_valid() {
        init_test_tracing();
        let settings = Settings::default();
        assert!(!settings.display_name.is_empty());
        assert!(!settings.storage_dir.as_os_str().is_empty());
        assert_eq!(settings.video_quality, Quality::Medium);
        assert_eq!(settings.audio_quality, Quality::Medium);
        assert_eq!(settings.theme, Theme::Dark);
    }

    #[test]
    fn save_and_load_roundtrip() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let dirs = test_dirs(tmp.path().to_path_buf());

        let settings = Settings {
            display_name: "Alice".to_string(),
            storage_dir: PathBuf::from("/tmp/bisc-test"),
            video_quality: Quality::High,
            audio_quality: Quality::Low,
            input_device: "USB Mic".to_string(),
            output_device: "Headphones".to_string(),
            theme: Theme::Light,
        };

        settings.save(&dirs).unwrap();
        let loaded = Settings::load(&dirs);

        assert_eq!(settings, loaded);
    }

    #[test]
    fn missing_config_returns_defaults() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("nonexistent");
        let dirs = test_dirs(config_dir);

        let loaded = Settings::load(&dirs);
        assert_eq!(loaded.video_quality, Quality::Medium);
        assert_eq!(loaded.theme, Theme::Dark);
    }

    #[test]
    fn corrupted_config_returns_defaults() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().to_path_buf();
        let dirs = test_dirs(config_dir.clone());

        // Write garbage to the settings file
        std::fs::write(config_dir.join("settings.toml"), "{{{{not valid toml}}}}").unwrap();

        let loaded = Settings::load(&dirs);
        assert_eq!(loaded.video_quality, Quality::Medium);
        assert_eq!(loaded.theme, Theme::Dark);
    }

    #[test]
    fn all_fields_serialize_correctly() {
        init_test_tracing();
        let settings = Settings {
            display_name: "Bob".to_string(),
            storage_dir: PathBuf::from("/data/bisc"),
            video_quality: Quality::Low,
            audio_quality: Quality::High,
            input_device: "Built-in Mic".to_string(),
            output_device: "".to_string(),
            theme: Theme::Light,
        };

        let toml_str = toml::to_string_pretty(&settings).unwrap();
        let deserialized: Settings = toml::from_str(&toml_str).unwrap();
        assert_eq!(settings, deserialized);

        // Verify the TOML contains expected fields
        assert!(toml_str.contains("display_name"));
        assert!(toml_str.contains("storage_dir"));
        assert!(toml_str.contains("video_quality"));
        assert!(toml_str.contains("audio_quality"));
        assert!(toml_str.contains("input_device"));
        assert!(toml_str.contains("output_device"));
        assert!(toml_str.contains("theme"));
    }

    #[test]
    fn data_dir_override_sets_custom_paths() {
        init_test_tracing();
        let data_dir = PathBuf::from("/tmp/bisc-instance-2");
        let dirs = AppDirs::resolve(Some(&data_dir));
        assert_eq!(
            dirs.config_dir,
            PathBuf::from("/tmp/bisc-instance-2/config")
        );
        assert_eq!(
            dirs.default_storage_dir,
            PathBuf::from("/tmp/bisc-instance-2/data")
        );
    }

    #[test]
    fn no_data_dir_override_uses_platform_defaults() {
        init_test_tracing();
        let dirs = AppDirs::resolve(None);
        // Should not be empty — either platform dirs or fallback
        assert!(!dirs.config_dir.as_os_str().is_empty());
        assert!(!dirs.default_storage_dir.as_os_str().is_empty());
    }

    #[test]
    fn save_and_load_with_data_dir_override() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let data_dir = tmp.path().to_path_buf();
        let dirs = AppDirs::resolve(Some(&data_dir));

        let settings = Settings {
            display_name: "Charlie".to_string(),
            storage_dir: dirs.default_storage_dir.clone(),
            video_quality: Quality::Medium,
            audio_quality: Quality::Medium,
            input_device: String::new(),
            output_device: String::new(),
            theme: Theme::Dark,
        };

        settings.save(&dirs).unwrap();
        let loaded = Settings::load(&dirs);
        assert_eq!(settings, loaded);
        assert_eq!(loaded.storage_dir, data_dir.join("data"));
    }
}
