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

impl Default for Settings {
    fn default() -> Self {
        let storage_dir = directories::ProjectDirs::from("", "", "bisc")
            .map(|d| d.data_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("bisc-data"));

        Self {
            display_name: "user".to_string(),
            storage_dir,
            video_quality: Quality::Medium,
            audio_quality: Quality::Medium,
            input_device: String::new(),
            output_device: String::new(),
            theme: Theme::Dark,
        }
    }
}

impl Settings {
    /// Load settings from the default config path.
    ///
    /// Returns defaults if the file doesn't exist or is corrupted.
    pub fn load() -> Self {
        Self::load_from_dir(Self::config_dir())
    }

    /// Save settings to the default config path.
    pub fn save(&self) -> Result<()> {
        self.save_to_dir(Self::config_dir())
    }

    /// Load settings from a specific config directory.
    pub fn load_from_dir(config_dir: PathBuf) -> Self {
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
                    Self::default()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tracing::info!(
                    path = %path.display(),
                    "settings file not found, using defaults"
                );
                Self::default()
            }
            Err(e) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %e,
                    "failed to read settings file, using defaults"
                );
                Self::default()
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

    /// Get the default config directory.
    fn config_dir() -> PathBuf {
        directories::ProjectDirs::from("", "", "bisc")
            .map(|d| d.config_dir().to_path_buf())
            .unwrap_or_else(|| PathBuf::from("bisc-config"))
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
        let config_dir = tmp.path().to_path_buf();

        let settings = Settings {
            display_name: "Alice".to_string(),
            storage_dir: PathBuf::from("/tmp/bisc-test"),
            video_quality: Quality::High,
            audio_quality: Quality::Low,
            input_device: "USB Mic".to_string(),
            output_device: "Headphones".to_string(),
            theme: Theme::Light,
        };

        settings.save_to_dir(config_dir.clone()).unwrap();
        let loaded = Settings::load_from_dir(config_dir);

        assert_eq!(settings, loaded);
    }

    #[test]
    fn missing_config_returns_defaults() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().join("nonexistent");

        let loaded = Settings::load_from_dir(config_dir);
        assert_eq!(loaded.video_quality, Quality::Medium);
        assert_eq!(loaded.theme, Theme::Dark);
    }

    #[test]
    fn corrupted_config_returns_defaults() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let config_dir = tmp.path().to_path_buf();

        // Write garbage to the settings file
        std::fs::write(config_dir.join("settings.toml"), "{{{{not valid toml}}}}").unwrap();

        let loaded = Settings::load_from_dir(config_dir);
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
}
