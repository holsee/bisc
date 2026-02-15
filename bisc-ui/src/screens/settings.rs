//! Settings screen: configure user preferences.

use iced::widget::{button, column, container, pick_list, radio, row, text, text_input};
use iced::{Alignment, Element, Length, Renderer, Theme};

/// Quality preset (matches bisc-app Settings).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    Low,
    Medium,
    High,
}

impl std::fmt::Display for Quality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Quality::Low => write!(f, "Low"),
            Quality::Medium => write!(f, "Medium"),
            Quality::High => write!(f, "High"),
        }
    }
}

/// UI theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeChoice {
    Light,
    Dark,
}

impl std::fmt::Display for ThemeChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ThemeChoice::Light => write!(f, "Light"),
            ThemeChoice::Dark => write!(f, "Dark"),
        }
    }
}

/// Messages emitted by the settings screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    DisplayNameChanged(String),
    StorageDirChanged(String),
    BrowseStorageDir,
    InputDeviceSelected(String),
    OutputDeviceSelected(String),
    VideoQualityChanged(Quality),
    AudioQualityChanged(Quality),
    ThemeChanged(ThemeChoice),
    Save,
    Back,
}

/// Result of processing a settings message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Save,
    Back,
    BrowseStorageDir,
}

/// Settings screen state.
#[derive(Debug, Clone)]
pub struct SettingsScreen {
    pub display_name: String,
    pub storage_dir: String,
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub video_quality: Quality,
    pub audio_quality: Quality,
    pub theme: ThemeChoice,
    /// Available input devices.
    pub input_devices: Vec<String>,
    /// Available output devices.
    pub output_devices: Vec<String>,
    /// Whether settings have been modified.
    pub dirty: bool,
}

impl Default for SettingsScreen {
    fn default() -> Self {
        Self {
            display_name: "user".to_string(),
            storage_dir: String::new(),
            input_device: None,
            output_device: None,
            video_quality: Quality::Medium,
            audio_quality: Quality::Medium,
            theme: ThemeChoice::Dark,
            input_devices: Vec::new(),
            output_devices: Vec::new(),
            dirty: false,
        }
    }
}

impl SettingsScreen {
    /// Create with available audio devices.
    pub fn with_devices(mut self, input_devices: Vec<String>, output_devices: Vec<String>) -> Self {
        self.input_devices = input_devices;
        self.output_devices = output_devices;
        self
    }

    /// Handle a message and return any external action.
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::DisplayNameChanged(name) => {
                self.display_name = name;
                self.dirty = true;
                Action::None
            }
            Message::StorageDirChanged(dir) => {
                self.storage_dir = dir;
                self.dirty = true;
                Action::None
            }
            Message::BrowseStorageDir => Action::BrowseStorageDir,
            Message::InputDeviceSelected(device) => {
                self.input_device = Some(device);
                self.dirty = true;
                Action::None
            }
            Message::OutputDeviceSelected(device) => {
                self.output_device = Some(device);
                self.dirty = true;
                Action::None
            }
            Message::VideoQualityChanged(q) => {
                self.video_quality = q;
                self.dirty = true;
                Action::None
            }
            Message::AudioQualityChanged(q) => {
                self.audio_quality = q;
                self.dirty = true;
                Action::None
            }
            Message::ThemeChanged(t) => {
                self.theme = t;
                self.dirty = true;
                Action::None
            }
            Message::Save => {
                self.dirty = false;
                tracing::info!(display_name = %self.display_name, "saving settings");
                Action::Save
            }
            Message::Back => Action::Back,
        }
    }

    /// Render the settings screen.
    pub fn view(&self) -> Element<'_, Message, Theme, Renderer> {
        let title = text("Settings").size(24);

        // Display name
        let name_label = text("Display Name").size(14);
        let name_input = text_input("Display name...", &self.display_name)
            .on_input(Message::DisplayNameChanged)
            .padding(8)
            .width(Length::Fixed(250.0));

        // Storage directory (editable)
        let storage_label = text("File Exchange Directory").size(14);
        let storage_input = text_input("Storage directory...", &self.storage_dir)
            .on_input(Message::StorageDirChanged)
            .padding(8)
            .width(Length::Fixed(250.0));
        let browse_btn = button(text("Browse")).on_press(Message::BrowseStorageDir);
        let storage_row = row![storage_input, browse_btn]
            .spacing(8)
            .align_y(Alignment::Center);

        // Audio input device
        let input_label = text("Audio Input").size(14);
        let input_picker: Element<'_, Message, Theme, Renderer> = pick_list(
            self.input_devices.clone(),
            self.input_device.clone(),
            Message::InputDeviceSelected,
        )
        .placeholder("System Default")
        .width(Length::Fixed(250.0))
        .into();

        // Audio output device
        let output_label = text("Audio Output").size(14);
        let output_picker: Element<'_, Message, Theme, Renderer> = pick_list(
            self.output_devices.clone(),
            self.output_device.clone(),
            Message::OutputDeviceSelected,
        )
        .placeholder("System Default")
        .width(Length::Fixed(250.0))
        .into();

        // Video quality
        let vq_label = text("Video Quality").size(14);
        let vq_row = row![
            radio(
                "Low",
                Quality::Low,
                Some(self.video_quality),
                Message::VideoQualityChanged
            ),
            radio(
                "Medium",
                Quality::Medium,
                Some(self.video_quality),
                Message::VideoQualityChanged
            ),
            radio(
                "High",
                Quality::High,
                Some(self.video_quality),
                Message::VideoQualityChanged
            ),
        ]
        .spacing(15);

        // Audio quality
        let aq_label = text("Audio Quality").size(14);
        let aq_row = row![
            radio(
                "Low",
                Quality::Low,
                Some(self.audio_quality),
                Message::AudioQualityChanged
            ),
            radio(
                "Medium",
                Quality::Medium,
                Some(self.audio_quality),
                Message::AudioQualityChanged
            ),
            radio(
                "High",
                Quality::High,
                Some(self.audio_quality),
                Message::AudioQualityChanged
            ),
        ]
        .spacing(15);

        // Theme
        let theme_label = text("Theme").size(14);
        let theme_row = row![
            radio(
                "Dark",
                ThemeChoice::Dark,
                Some(self.theme),
                Message::ThemeChanged
            ),
            radio(
                "Light",
                ThemeChoice::Light,
                Some(self.theme),
                Message::ThemeChanged
            ),
        ]
        .spacing(15);

        // Buttons
        let save_label = if self.dirty { "Save *" } else { "Save" };
        let buttons = row![
            button(text("Back")).on_press(Message::Back),
            button(text(save_label)).on_press(Message::Save),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let content = column![
            title,
            name_label,
            name_input,
            storage_label,
            storage_row,
            input_label,
            input_picker,
            output_label,
            output_picker,
            vq_label,
            vq_row,
            aq_label,
            aq_row,
            theme_label,
            theme_row,
            buttons,
        ]
        .spacing(8)
        .padding(30);

        container(content)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state() {
        let screen = SettingsScreen::default();
        assert_eq!(screen.display_name, "user");
        assert_eq!(screen.video_quality, Quality::Medium);
        assert_eq!(screen.audio_quality, Quality::Medium);
        assert_eq!(screen.theme, ThemeChoice::Dark);
        assert!(!screen.dirty);
    }

    #[test]
    fn change_display_name_marks_dirty() {
        let mut screen = SettingsScreen::default();
        screen.update(Message::DisplayNameChanged("Alice".to_string()));
        assert_eq!(screen.display_name, "Alice");
        assert!(screen.dirty);
    }

    #[test]
    fn save_clears_dirty() {
        let mut screen = SettingsScreen::default();
        screen.update(Message::DisplayNameChanged("Alice".to_string()));
        assert!(screen.dirty);

        let action = screen.update(Message::Save);
        assert_eq!(action, Action::Save);
        assert!(!screen.dirty);
    }

    #[test]
    fn quality_selectors_update() {
        let mut screen = SettingsScreen::default();
        screen.update(Message::VideoQualityChanged(Quality::High));
        assert_eq!(screen.video_quality, Quality::High);

        screen.update(Message::AudioQualityChanged(Quality::Low));
        assert_eq!(screen.audio_quality, Quality::Low);
    }

    #[test]
    fn theme_change() {
        let mut screen = SettingsScreen::default();
        screen.update(Message::ThemeChanged(ThemeChoice::Light));
        assert_eq!(screen.theme, ThemeChoice::Light);
    }

    #[test]
    fn device_selection() {
        let mut screen = SettingsScreen::default()
            .with_devices(vec!["Mic 1".to_string()], vec!["Speaker 1".to_string()]);

        screen.update(Message::InputDeviceSelected("Mic 1".to_string()));
        assert_eq!(screen.input_device, Some("Mic 1".to_string()));

        screen.update(Message::OutputDeviceSelected("Speaker 1".to_string()));
        assert_eq!(screen.output_device, Some("Speaker 1".to_string()));
    }

    #[test]
    fn back_action() {
        let mut screen = SettingsScreen::default();
        let action = screen.update(Message::Back);
        assert_eq!(action, Action::Back);
    }
}
