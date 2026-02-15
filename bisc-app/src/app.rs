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
    /// Channel was successfully created (ticket received).
    ChannelCreated(String),
    /// Successfully joined a channel.
    ChannelJoined,
    /// Error during channel operation.
    ChannelError(String),
    /// A peer joined the channel (from subscription).
    PeerJoined {
        name: String,
        id: String,
        mic_enabled: bool,
        camera_enabled: bool,
        screen_sharing: bool,
    },
    /// A peer left the channel (from subscription).
    PeerLeft(String),
    /// A direct connection was established to a peer (from subscription).
    PeerConnected(String),
    /// A peer's media state changed (from subscription).
    MediaStateChanged {
        peer_id: String,
        audio_muted: bool,
        video_enabled: bool,
        screen_sharing: bool,
        app_audio_sharing: bool,
    },
    /// A file was announced by a peer (from subscription).
    FileAnnounced {
        sender_id: String,
        hash: String,
        name: String,
        size: u64,
    },
    /// A file was successfully shared (local user).
    FileShared {
        hash: String,
        name: String,
        size: u64,
    },
    /// File picker was cancelled by the user.
    FileShareCancelled,
    /// File sharing failed.
    FileShareFailed(String),
    /// File download completed.
    FileDownloadComplete(String),
    /// File download failed.
    FileDownloadFailed { hash: String, error: String },
    /// Periodic tick to sync video frames from pipelines to the UI.
    VideoFrameTick,
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
        let storage_dir_str = settings.storage_dir.to_string_lossy().to_string();
        Self {
            screen: Screen::Channel,
            previous_screen: Screen::Channel,
            channel_screen: ChannelScreen::new(
                settings.display_name.clone(),
                storage_dir_str.clone(),
            ),
            call_screen: None,
            files_panel: FilesPanel::default(),
            settings_screen: SettingsScreen {
                display_name: settings.display_name,
                storage_dir: storage_dir_str,
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
    /// Toggle app audio sharing.
    SetAppAudio(bool),
    /// Open file picker.
    OpenFilePicker,
    /// Download a file.
    DownloadFile(String),
    /// Save settings.
    SaveSettings,
    /// Browse for storage directory.
    BrowseStorageDir,
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
                    channel::Action::OpenSettings => {
                        self.previous_screen = self.screen.clone();
                        self.screen = Screen::Settings;
                        AppAction::None
                    }
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
                        call::Action::SetAppAudio(on) => AppAction::SetAppAudio(on),
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
                        // Sync storage_dir back to channel screen when going back
                        self.channel_screen.storage_dir = self.settings_screen.storage_dir.clone();
                        self.screen = self.previous_screen.clone();
                        AppAction::None
                    }
                    settings::Action::BrowseStorageDir => AppAction::BrowseStorageDir,
                }
            }
            AppMessage::OpenSettings => {
                self.previous_screen = self.screen.clone();
                self.screen = Screen::Settings;
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
            AppMessage::PeerJoined {
                name,
                id,
                mic_enabled,
                camera_enabled,
                screen_sharing,
            } => {
                if let Some(call) = &mut self.call_screen {
                    call.peer_joined(call::PeerInfo {
                        name,
                        id,
                        mic_enabled,
                        camera_enabled,
                        screen_sharing,
                        app_audio_sharing: false,
                    });
                }
                AppAction::None
            }
            AppMessage::PeerLeft(id) => {
                if let Some(call) = &mut self.call_screen {
                    call.peer_left(&id);
                }
                AppAction::None
            }
            AppMessage::PeerConnected(id) => {
                tracing::info!(peer_id = %id, "peer connected (direct QUIC)");
                AppAction::None
            }
            AppMessage::MediaStateChanged {
                peer_id,
                audio_muted,
                video_enabled,
                screen_sharing,
                app_audio_sharing,
            } => {
                if let Some(call) = &mut self.call_screen {
                    call.update_peer_media(
                        &peer_id,
                        !audio_muted,
                        video_enabled,
                        screen_sharing,
                        app_audio_sharing,
                    );
                }
                AppAction::None
            }
            AppMessage::FileAnnounced {
                sender_id,
                hash,
                name,
                size,
            } => {
                self.files_panel.file_announced(hash, name, size, sender_id);
                AppAction::None
            }
            AppMessage::FileShared { hash, name, size } => {
                // Add to our files panel as already downloaded (we have it)
                self.files_panel
                    .file_announced(hash.clone(), name, size, "You".to_string());
                self.files_panel.file_completed(&hash);
                AppAction::None
            }
            AppMessage::FileShareCancelled => AppAction::None,
            AppMessage::FileShareFailed(error) => {
                tracing::error!(error = %error, "file sharing failed");
                AppAction::None
            }
            AppMessage::FileDownloadComplete(hash) => {
                self.files_panel.file_completed(&hash);
                AppAction::None
            }
            AppMessage::FileDownloadFailed { hash, error } => {
                self.files_panel.download_failed(&hash, error);
                AppAction::None
            }
            // VideoFrameTick is handled directly by BiscApp before reaching here.
            AppMessage::VideoFrameTick => AppAction::None,
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

    // --- Channel event subscription flow tests ---

    /// Helper: create an app with an active call screen.
    fn app_in_call() -> App {
        let mut app = App::default();
        app.update(AppMessage::ChannelCreated("ticket".to_string()));
        assert_eq!(app.screen, Screen::Call);
        app
    }

    #[test]
    fn peer_joined_adds_to_call_screen() {
        let mut app = app_in_call();
        app.update(AppMessage::PeerJoined {
            name: "Bob".to_string(),
            id: "abcd1234".to_string(),
            mic_enabled: true,
            camera_enabled: false,
            screen_sharing: false,
        });

        let call = app.call_screen.as_ref().unwrap();
        assert_eq!(call.peers.len(), 1);
        assert_eq!(call.peers[0].name, "Bob");
        assert_eq!(call.peers[0].id, "abcd1234");
        assert!(call.peers[0].mic_enabled);
        assert!(!call.peers[0].camera_enabled);
    }

    #[test]
    fn peer_left_removes_from_call_screen() {
        let mut app = app_in_call();
        app.update(AppMessage::PeerJoined {
            name: "Bob".to_string(),
            id: "abcd1234".to_string(),
            mic_enabled: true,
            camera_enabled: false,
            screen_sharing: false,
        });
        assert_eq!(app.call_screen.as_ref().unwrap().peers.len(), 1);

        app.update(AppMessage::PeerLeft("abcd1234".to_string()));
        assert!(app.call_screen.as_ref().unwrap().peers.is_empty());
    }

    #[test]
    fn media_state_changed_updates_peer() {
        let mut app = app_in_call();
        app.update(AppMessage::PeerJoined {
            name: "Bob".to_string(),
            id: "abcd1234".to_string(),
            mic_enabled: true,
            camera_enabled: false,
            screen_sharing: false,
        });

        app.update(AppMessage::MediaStateChanged {
            peer_id: "abcd1234".to_string(),
            audio_muted: true,
            video_enabled: true,
            screen_sharing: true,
            app_audio_sharing: false,
        });

        let peer = &app.call_screen.as_ref().unwrap().peers[0];
        assert!(!peer.mic_enabled); // audio_muted=true → mic_enabled=false
        assert!(peer.camera_enabled);
        assert!(peer.screen_sharing);
    }

    #[test]
    fn file_announced_adds_to_files_panel() {
        let mut app = app_in_call();
        app.update(AppMessage::FileAnnounced {
            sender_id: "sender1".to_string(),
            hash: "abc123".to_string(),
            name: "document.pdf".to_string(),
            size: 4096,
        });

        assert_eq!(app.files_panel.files.len(), 1);
        assert_eq!(app.files_panel.files[0].name, "document.pdf");
        assert_eq!(app.files_panel.files[0].size, 4096);
        assert_eq!(app.files_panel.files[0].sender, "sender1");
    }

    #[test]
    fn peer_events_ignored_without_call_screen() {
        let mut app = App::default();
        // No call screen — events should be silently handled (not panic)
        app.update(AppMessage::PeerJoined {
            name: "Bob".to_string(),
            id: "abcd1234".to_string(),
            mic_enabled: true,
            camera_enabled: false,
            screen_sharing: false,
        });
        app.update(AppMessage::PeerLeft("abcd1234".to_string()));
        app.update(AppMessage::MediaStateChanged {
            peer_id: "abcd1234".to_string(),
            audio_muted: false,
            video_enabled: false,
            screen_sharing: false,
            app_audio_sharing: false,
        });
        // Files panel always exists, so file announcements still work
        app.update(AppMessage::FileAnnounced {
            sender_id: "sender1".to_string(),
            hash: "abc".to_string(),
            name: "test.txt".to_string(),
            size: 100,
        });
        assert_eq!(app.files_panel.files.len(), 1);
    }

    #[test]
    fn all_channel_event_variants_map_to_ui_update() {
        let mut app = app_in_call();

        // PeerJoined → adds peer
        app.update(AppMessage::PeerJoined {
            name: "Alice".to_string(),
            id: "a1".to_string(),
            mic_enabled: true,
            camera_enabled: true,
            screen_sharing: false,
        });
        assert_eq!(app.call_screen.as_ref().unwrap().peers.len(), 1);

        // PeerConnected → logged (no state change expected)
        let action = app.update(AppMessage::PeerConnected("a1".to_string()));
        assert_eq!(action, AppAction::None);

        // MediaStateChanged → updates peer
        app.update(AppMessage::MediaStateChanged {
            peer_id: "a1".to_string(),
            audio_muted: true,
            video_enabled: false,
            screen_sharing: true,
            app_audio_sharing: false,
        });
        let peer = &app.call_screen.as_ref().unwrap().peers[0];
        assert!(!peer.mic_enabled);
        assert!(!peer.camera_enabled);
        assert!(peer.screen_sharing);

        // FileAnnounced → adds file
        app.update(AppMessage::FileAnnounced {
            sender_id: "a1".to_string(),
            hash: "hash1".to_string(),
            name: "file.zip".to_string(),
            size: 999,
        });
        assert_eq!(app.files_panel.files.len(), 1);

        // PeerLeft → removes peer
        app.update(AppMessage::PeerLeft("a1".to_string()));
        assert!(app.call_screen.as_ref().unwrap().peers.is_empty());
    }
}
