//! Top-level Iced application shell with navigation between screens.
//!
//! The Elm architecture state machine: `Screen` enum tracks which screen
//! is active, and `update()` handles messages from all screens.

use bisc_ui::screens::call::{self, CallScreen};
use bisc_ui::screens::channel::{self, ChannelScreen};
use bisc_ui::screens::files::{self, FilesPanel};
use bisc_ui::screens::settings::{self, SettingsScreen};

/// Navigation state: which screen is currently displayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Channel,
    Call,
    Settings,
}

/// Top-level application message.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Variants constructed by networking layer (BISC-036)
pub enum AppMessage {
    /// Messages from the channel screen.
    Channel(channel::Message),
    /// Messages from the call screen.
    Call(call::Message),
    /// Messages from the files panel.
    Files(files::Message),
    /// Messages from the settings screen.
    Settings(settings::Message),
    /// Navigation: go to settings.
    OpenSettings,
    /// Navigation: go back from settings.
    CloseSettings,
    /// Channel was successfully created (ticket received).
    ChannelCreated(String),
    /// Successfully joined a channel.
    ChannelJoined,
    /// Error during channel operation.
    ChannelError(String),
}

/// Top-level application state.
pub struct App {
    /// Current screen.
    pub screen: Screen,
    /// Previous screen (for back navigation from settings).
    pub previous_screen: Screen,
    /// Channel screen state.
    pub channel_screen: ChannelScreen,
    /// Call screen state (created when joining a channel).
    pub call_screen: Option<CallScreen>,
    /// Files panel state.
    pub files_panel: FilesPanel,
    /// Settings screen state.
    pub settings_screen: SettingsScreen,
}

impl Default for App {
    fn default() -> Self {
        let settings = crate::settings::Settings::default();
        Self {
            screen: Screen::Channel,
            previous_screen: Screen::Channel,
            channel_screen: ChannelScreen::new(settings.display_name.clone()),
            call_screen: None,
            files_panel: FilesPanel::default(),
            settings_screen: SettingsScreen {
                display_name: settings.display_name,
                storage_dir: settings.storage_dir.to_string_lossy().to_string(),
                ..SettingsScreen::default()
            },
        }
    }
}

/// Result of processing an app message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    None,
    /// Create a channel with the given display name.
    CreateChannel(String),
    /// Join a channel with ticket + display name.
    JoinChannel {
        ticket: String,
        display_name: String,
    },
    /// Leave the current channel.
    LeaveChannel,
    /// Copy text to clipboard.
    CopyToClipboard(String),
    /// Toggle microphone (true = enable).
    SetMic(bool),
    /// Toggle camera (true = enable).
    SetCamera(bool),
    /// Toggle screen share.
    SetScreenShare(bool),
    /// Open file picker.
    OpenFilePicker,
    /// Download a file.
    DownloadFile(String),
    /// Save settings.
    SaveSettings,
}

impl App {
    /// Handle a top-level message and return an action.
    pub fn update(&mut self, message: AppMessage) -> AppAction {
        match message {
            AppMessage::Channel(msg) => {
                let action = self.channel_screen.update(msg);
                match action {
                    channel::Action::None => AppAction::None,
                    channel::Action::CreateChannel { display_name } => {
                        AppAction::CreateChannel(display_name)
                    }
                    channel::Action::JoinChannel {
                        ticket,
                        display_name,
                    } => AppAction::JoinChannel {
                        ticket,
                        display_name,
                    },
                    channel::Action::CopyToClipboard(text) => AppAction::CopyToClipboard(text),
                }
            }
            AppMessage::Call(msg) => {
                if let Some(call) = &mut self.call_screen {
                    let action = call.update(msg);
                    match action {
                        call::Action::None => AppAction::None,
                        call::Action::SetMic(on) => AppAction::SetMic(on),
                        call::Action::SetCamera(on) => AppAction::SetCamera(on),
                        call::Action::SetScreenShare(on) => AppAction::SetScreenShare(on),
                        call::Action::OpenFilePicker => AppAction::OpenFilePicker,
                        call::Action::LeaveChannel => {
                            self.screen = Screen::Channel;
                            self.call_screen = None;
                            AppAction::LeaveChannel
                        }
                        call::Action::CopyToClipboard(text) => AppAction::CopyToClipboard(text),
                    }
                } else {
                    AppAction::None
                }
            }
            AppMessage::Files(msg) => {
                let action = self.files_panel.update(msg);
                match action {
                    files::Action::None => AppAction::None,
                    files::Action::Download(hash) => AppAction::DownloadFile(hash),
                    files::Action::OpenFilePicker => AppAction::OpenFilePicker,
                }
            }
            AppMessage::Settings(msg) => {
                let action = self.settings_screen.update(msg);
                match action {
                    settings::Action::None => AppAction::None,
                    settings::Action::Save => AppAction::SaveSettings,
                    settings::Action::Back => {
                        self.screen = self.previous_screen.clone();
                        AppAction::None
                    }
                }
            }
            AppMessage::OpenSettings => {
                self.previous_screen = self.screen.clone();
                self.screen = Screen::Settings;
                AppAction::None
            }
            AppMessage::CloseSettings => {
                self.screen = self.previous_screen.clone();
                AppAction::None
            }
            AppMessage::ChannelCreated(ticket) => {
                self.channel_screen.set_ticket(ticket.clone());
                self.call_screen = Some(CallScreen::new(
                    self.channel_screen.display_name.clone(),
                    ticket,
                ));
                self.screen = Screen::Call;
                AppAction::None
            }
            AppMessage::ChannelJoined => {
                self.call_screen = Some(CallScreen::new(
                    self.channel_screen.display_name.clone(),
                    String::new(),
                ));
                self.screen = Screen::Call;
                AppAction::None
            }
            AppMessage::ChannelError(error) => {
                self.channel_screen.set_error(error);
                AppAction::None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_on_channel_screen() {
        let app = App::default();
        assert_eq!(app.screen, Screen::Channel);
        assert!(app.call_screen.is_none());
    }

    #[test]
    fn channel_created_transitions_to_call() {
        let mut app = App::default();
        app.update(AppMessage::ChannelCreated("ticket123".to_string()));
        assert_eq!(app.screen, Screen::Call);
        assert!(app.call_screen.is_some());
    }

    #[test]
    fn channel_joined_transitions_to_call() {
        let mut app = App::default();
        app.update(AppMessage::ChannelJoined);
        assert_eq!(app.screen, Screen::Call);
        assert!(app.call_screen.is_some());
    }

    #[test]
    fn leave_channel_returns_to_channel_screen() {
        let mut app = App::default();
        app.update(AppMessage::ChannelCreated("t".to_string()));
        assert_eq!(app.screen, Screen::Call);

        let action = app.update(AppMessage::Call(call::Message::LeaveChannel));
        assert_eq!(action, AppAction::LeaveChannel);
        assert_eq!(app.screen, Screen::Channel);
        assert!(app.call_screen.is_none());
    }

    #[test]
    fn settings_navigation() {
        let mut app = App::default();

        // Open settings from channel screen
        app.update(AppMessage::OpenSettings);
        assert_eq!(app.screen, Screen::Settings);
        assert_eq!(app.previous_screen, Screen::Channel);

        // Back goes to channel
        app.update(AppMessage::Settings(settings::Message::Back));
        assert_eq!(app.screen, Screen::Channel);
    }

    #[test]
    fn settings_from_call_returns_to_call() {
        let mut app = App::default();
        app.update(AppMessage::ChannelCreated("t".to_string()));
        assert_eq!(app.screen, Screen::Call);

        app.update(AppMessage::OpenSettings);
        assert_eq!(app.screen, Screen::Settings);
        assert_eq!(app.previous_screen, Screen::Call);

        app.update(AppMessage::Settings(settings::Message::Back));
        assert_eq!(app.screen, Screen::Call);
    }

    #[test]
    fn channel_error_shows_on_channel_screen() {
        let mut app = App::default();
        app.update(AppMessage::ChannelError("bad ticket".to_string()));
        assert_eq!(app.screen, Screen::Channel);
        assert!(app.channel_screen.error.is_some());
    }

    #[test]
    fn save_settings_action() {
        let mut app = App::default();
        app.update(AppMessage::OpenSettings);
        let action = app.update(AppMessage::Settings(settings::Message::Save));
        assert_eq!(action, AppAction::SaveSettings);
    }

    #[test]
    fn mic_toggle_in_call() {
        let mut app = App::default();
        app.update(AppMessage::ChannelCreated("t".to_string()));

        let action = app.update(AppMessage::Call(call::Message::ToggleMic));
        assert_eq!(action, AppAction::SetMic(false));
    }
}
