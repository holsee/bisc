//! Call screen: peer list, video grid, and media controls.
//!
//! The main screen shown during an active call.

use std::collections::HashMap;

use iced::widget::{button, column, container, row, scrollable, shader, text};
use iced::{Alignment, Element, Length, Renderer, Theme};

use crate::video_grid;
use crate::video_surface::VideoSurface;

/// Peer state shown in the sidebar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    /// Display name.
    pub name: String,
    /// Unique peer identifier (hex-encoded).
    pub id: String,
    /// Whether the peer has their microphone enabled.
    pub mic_enabled: bool,
    /// Whether the peer has their camera enabled.
    pub camera_enabled: bool,
    /// Whether the peer is sharing their screen.
    pub screen_sharing: bool,
}

/// Messages emitted by the call screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Toggle local microphone.
    ToggleMic,
    /// Toggle local camera.
    ToggleCamera,
    /// Start or stop screen sharing.
    ToggleScreenShare,
    /// Open file picker to share a file.
    ShareFile,
    /// Leave the current channel.
    LeaveChannel,
    /// Copy invite ticket to clipboard.
    CopyTicket,
}

/// Local media state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalMediaState {
    pub mic_enabled: bool,
    pub camera_enabled: bool,
    pub screen_sharing: bool,
}

impl Default for LocalMediaState {
    fn default() -> Self {
        Self {
            mic_enabled: true,
            camera_enabled: false,
            screen_sharing: false,
        }
    }
}

/// Result of processing a call screen message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No external action needed.
    None,
    /// Toggle microphone (true = enable).
    SetMic(bool),
    /// Toggle camera (true = enable).
    SetCamera(bool),
    /// Toggle screen share (true = start).
    SetScreenShare(bool),
    /// Open file picker for sharing.
    OpenFilePicker,
    /// Leave the channel.
    LeaveChannel,
    /// Copy text to clipboard.
    CopyToClipboard(String),
}

/// State of the call screen.
#[derive(Debug, Clone)]
pub struct CallScreen {
    /// Connected peers.
    pub peers: Vec<PeerInfo>,
    /// Local media state.
    pub local_media: LocalMediaState,
    /// Channel invite ticket.
    pub ticket: String,
    /// Our display name.
    pub display_name: String,
    /// Video surfaces for remote peers (keyed by peer hex ID).
    video_surfaces: HashMap<String, VideoSurface>,
    /// Local camera preview surface.
    local_surface: VideoSurface,
}

impl CallScreen {
    /// Create a new call screen.
    pub fn new(display_name: String, ticket: String) -> Self {
        Self {
            peers: Vec::new(),
            local_media: LocalMediaState::default(),
            ticket,
            display_name,
            video_surfaces: HashMap::new(),
            local_surface: VideoSurface::new(),
        }
    }

    /// Add a peer to the call.
    pub fn peer_joined(&mut self, peer: PeerInfo) {
        if !self.peers.iter().any(|p| p.id == peer.id) {
            tracing::info!(peer_name = %peer.name, peer_id = %peer.id, "peer joined call");
            self.peers.push(peer);
        }
    }

    /// Remove a peer from the call.
    pub fn peer_left(&mut self, peer_id: &str) {
        if let Some(pos) = self.peers.iter().position(|p| p.id == peer_id) {
            let removed = self.peers.remove(pos);
            tracing::info!(peer_name = %removed.name, peer_id = %removed.id, "peer left call");
        }
    }

    /// Update a peer's media state.
    pub fn update_peer_media(
        &mut self,
        peer_id: &str,
        mic: bool,
        camera: bool,
        screen_sharing: bool,
    ) {
        if let Some(peer) = self.peers.iter_mut().find(|p| p.id == peer_id) {
            peer.mic_enabled = mic;
            peer.camera_enabled = camera;
            peer.screen_sharing = screen_sharing;
        }
    }

    /// Update a remote peer's video frame.
    pub fn update_video_frame(&mut self, peer_id: &str, width: u32, height: u32, rgba_data: &[u8]) {
        let surface = self.video_surfaces.entry(peer_id.to_string()).or_default();
        surface.update_frame(width, height, rgba_data);
    }

    /// Update the local camera preview frame.
    pub fn update_local_preview(&mut self, width: u32, height: u32, rgba_data: &[u8]) {
        self.local_surface.update_frame(width, height, rgba_data);
    }

    /// Remove a peer's video surface.
    pub fn clear_video_frame(&mut self, peer_id: &str) {
        self.video_surfaces.remove(peer_id);
    }

    /// Clear the local camera preview.
    pub fn clear_local_preview(&mut self) {
        self.local_surface = VideoSurface::new();
    }

    /// Build the video area: a grid of VideoSurface widgets for active streams,
    /// or a placeholder if no video is active.
    fn build_video_area(&self) -> Element<'_, Message, Theme, Renderer> {
        // Collect surfaces with active frames: local preview + remote peers
        let has_local = self.local_media.camera_enabled && self.local_surface.has_frame();
        let remote_count = self.video_surfaces.len();
        let total = remote_count + if has_local { 1 } else { 0 };

        if total == 0 {
            // No video streams active — show placeholder
            let active_cameras = self.peers.iter().filter(|p| p.camera_enabled).count();
            let status = if active_cameras > 0 {
                format!("{active_cameras} peer(s) with camera enabled")
            } else {
                format!("{} peer(s) connected", self.peers.len())
            };
            return container(text(status).size(16))
                .center_x(Length::Fill)
                .center_y(Length::Fill)
                .into();
        }

        // Build grid using rows and columns
        let (cols, _rows) = video_grid::grid_dims(total);
        let cols = cols.max(1);

        // Collect all surfaces: local first, then remotes
        let mut surface_elements: Vec<Element<'_, Message, Theme, Renderer>> =
            Vec::with_capacity(total);

        if has_local {
            surface_elements.push(
                container(
                    shader(&self.local_surface)
                        .width(Length::Fill)
                        .height(Length::Fill),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            );
        }

        for surface in self.video_surfaces.values() {
            surface_elements.push(
                container(shader(surface).width(Length::Fill).height(Length::Fill))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            );
        }

        // Arrange into rows of `cols` elements
        let mut grid = column![].spacing(2);
        for chunk in surface_elements.chunks_mut(cols) {
            let mut r = row![].spacing(2).height(Length::Fill);
            for el in chunk.iter_mut() {
                // Take the element out, replacing with a placeholder
                let taken = std::mem::replace(
                    el,
                    container(text(""))
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into(),
                );
                r = r.push(taken);
            }
            grid = grid.push(r);
        }

        grid.width(Length::Fill).height(Length::Fill).into()
    }

    /// Handle a message and return any external action.
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::ToggleMic => {
                self.local_media.mic_enabled = !self.local_media.mic_enabled;
                tracing::info!(mic = self.local_media.mic_enabled, "toggled mic");
                Action::SetMic(self.local_media.mic_enabled)
            }
            Message::ToggleCamera => {
                self.local_media.camera_enabled = !self.local_media.camera_enabled;
                tracing::info!(camera = self.local_media.camera_enabled, "toggled camera");
                Action::SetCamera(self.local_media.camera_enabled)
            }
            Message::ToggleScreenShare => {
                self.local_media.screen_sharing = !self.local_media.screen_sharing;
                tracing::info!(
                    sharing = self.local_media.screen_sharing,
                    "toggled screen share"
                );
                Action::SetScreenShare(self.local_media.screen_sharing)
            }
            Message::ShareFile => Action::OpenFilePicker,
            Message::LeaveChannel => Action::LeaveChannel,
            Message::CopyTicket => Action::CopyToClipboard(self.ticket.clone()),
        }
    }

    /// Render the call screen.
    pub fn view(&self) -> Element<'_, Message, Theme, Renderer> {
        // Sidebar: peer list
        let mut peer_list = column![text("Peers").size(16)].spacing(5);
        for peer in &self.peers {
            let status = format!(
                "{} {}{}{}",
                peer.name,
                if peer.mic_enabled { "🎤" } else { "🔇" },
                if peer.camera_enabled { "📹" } else { "" },
                if peer.screen_sharing { "🖥" } else { "" },
            );
            peer_list = peer_list.push(text(status).size(13));
        }
        if self.peers.is_empty() {
            peer_list = peer_list.push(text("No peers connected").size(12));
        }

        let sidebar = container(
            column![
                scrollable(peer_list).height(Length::Fill),
                button(text("Copy Invite")).on_press(Message::CopyTicket),
            ]
            .spacing(10)
            .padding(10),
        )
        .width(Length::Fixed(180.0))
        .height(Length::Fill);

        // Video area: render VideoSurface widgets in a grid layout
        let video_area = self.build_video_area();

        // Control bar
        let mic_label = if self.local_media.mic_enabled {
            "Mute"
        } else {
            "Unmute"
        };
        let camera_label = if self.local_media.camera_enabled {
            "Camera Off"
        } else {
            "Camera On"
        };
        let share_label = if self.local_media.screen_sharing {
            "Stop Share"
        } else {
            "Share Screen"
        };

        let controls = row![
            button(text(mic_label)).on_press(Message::ToggleMic),
            button(text(camera_label)).on_press(Message::ToggleCamera),
            button(text(share_label)).on_press(Message::ToggleScreenShare),
            button(text("Share File")).on_press(Message::ShareFile),
            button(text("Leave")).on_press(Message::LeaveChannel),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .padding(10);

        let main_content = column![row![sidebar, video_area].height(Length::Fill), controls,];

        container(main_content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_peer(name: &str, id: &str) -> PeerInfo {
        PeerInfo {
            name: name.to_string(),
            id: id.to_string(),
            mic_enabled: true,
            camera_enabled: false,
            screen_sharing: false,
        }
    }

    #[test]
    fn initial_state() {
        let screen = CallScreen::new("Alice".to_string(), "ticket123".to_string());
        assert!(screen.peers.is_empty());
        assert!(screen.local_media.mic_enabled);
        assert!(!screen.local_media.camera_enabled);
        assert!(!screen.local_media.screen_sharing);
    }

    #[test]
    fn peer_join_and_leave() {
        let mut screen = CallScreen::new("Alice".to_string(), "t".to_string());

        screen.peer_joined(test_peer("Bob", "b1"));
        assert_eq!(screen.peers.len(), 1);

        screen.peer_joined(test_peer("Carol", "c1"));
        assert_eq!(screen.peers.len(), 2);

        // Duplicate join is ignored
        screen.peer_joined(test_peer("Bob Again", "b1"));
        assert_eq!(screen.peers.len(), 2);

        screen.peer_left("b1");
        assert_eq!(screen.peers.len(), 1);
        assert_eq!(screen.peers[0].name, "Carol");

        // Leave unknown peer is a no-op
        screen.peer_left("unknown");
        assert_eq!(screen.peers.len(), 1);
    }

    #[test]
    fn toggle_mic() {
        let mut screen = CallScreen::new("Alice".to_string(), "t".to_string());
        assert!(screen.local_media.mic_enabled);

        let action = screen.update(Message::ToggleMic);
        assert_eq!(action, Action::SetMic(false));
        assert!(!screen.local_media.mic_enabled);

        let action = screen.update(Message::ToggleMic);
        assert_eq!(action, Action::SetMic(true));
        assert!(screen.local_media.mic_enabled);
    }

    #[test]
    fn toggle_camera() {
        let mut screen = CallScreen::new("Alice".to_string(), "t".to_string());
        assert!(!screen.local_media.camera_enabled);

        let action = screen.update(Message::ToggleCamera);
        assert_eq!(action, Action::SetCamera(true));
        assert!(screen.local_media.camera_enabled);
    }

    #[test]
    fn toggle_screen_share() {
        let mut screen = CallScreen::new("Alice".to_string(), "t".to_string());
        let action = screen.update(Message::ToggleScreenShare);
        assert_eq!(action, Action::SetScreenShare(true));
        assert!(screen.local_media.screen_sharing);
    }

    #[test]
    fn leave_channel() {
        let mut screen = CallScreen::new("Alice".to_string(), "t".to_string());
        let action = screen.update(Message::LeaveChannel);
        assert_eq!(action, Action::LeaveChannel);
    }

    #[test]
    fn share_file() {
        let mut screen = CallScreen::new("Alice".to_string(), "t".to_string());
        let action = screen.update(Message::ShareFile);
        assert_eq!(action, Action::OpenFilePicker);
    }

    #[test]
    fn copy_ticket() {
        let mut screen = CallScreen::new("Alice".to_string(), "my-ticket".to_string());
        let action = screen.update(Message::CopyTicket);
        assert_eq!(action, Action::CopyToClipboard("my-ticket".to_string()));
    }

    #[test]
    fn update_peer_media_state() {
        let mut screen = CallScreen::new("Alice".to_string(), "t".to_string());
        screen.peer_joined(test_peer("Bob", "b1"));

        screen.update_peer_media("b1", false, true, true);
        let bob = &screen.peers[0];
        assert!(!bob.mic_enabled);
        assert!(bob.camera_enabled);
        assert!(bob.screen_sharing);
    }
}
