//! File sharing panel: view, download, and share files.

use iced::widget::{button, column, container, progress_bar, row, scrollable, text};
use iced::{Alignment, Element, Length, Renderer, Theme};

/// Status of a file in the sharing list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// Available for download from peers.
    Available,
    /// Currently downloading.
    Downloading {
        chunks_received: u32,
        total_chunks: u32,
    },
    /// Fully downloaded and verified.
    Downloaded,
    /// Download failed.
    Failed(String),
}

/// A file entry in the sharing panel.
#[derive(Debug, Clone)]
pub struct SharedFile {
    /// File hash (hex-encoded).
    pub hash: String,
    /// File name.
    pub name: String,
    /// File size in bytes.
    pub size: u64,
    /// Who shared the file.
    pub sender: String,
    /// Current status.
    pub status: FileStatus,
    /// Peer IDs that have this file.
    pub available_from: Vec<String>,
}

/// Messages emitted by the files panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    /// Request to download a file by hash.
    RequestDownload(String),
    /// Open file picker to share a file.
    ShareFile,
}

/// Result of processing a files panel message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    /// Download a file by hash.
    Download(String),
    /// Open file picker.
    OpenFilePicker,
}

/// File sharing panel state.
#[derive(Debug, Clone, Default)]
pub struct FilesPanel {
    /// Known shared files.
    pub files: Vec<SharedFile>,
    /// Error message (e.g. file store not initialized).
    pub error: Option<String>,
}

impl FilesPanel {
    /// Add a newly announced file.
    pub fn file_announced(&mut self, hash: String, name: String, size: u64, sender: String) {
        if !self.files.iter().any(|f| f.hash == hash) {
            tracing::info!(
                file_name = %name,
                file_hash = %hash,
                sender = %sender,
                "file announced"
            );
            self.files.push(SharedFile {
                hash,
                name,
                size,
                sender,
                status: FileStatus::Available,
                available_from: Vec::new(),
            });
        }
    }

    /// Update download progress for a file.
    pub fn update_progress(&mut self, hash: &str, chunks_received: u32, total_chunks: u32) {
        if let Some(file) = self.files.iter_mut().find(|f| f.hash == hash) {
            if chunks_received >= total_chunks {
                file.status = FileStatus::Downloaded;
            } else {
                file.status = FileStatus::Downloading {
                    chunks_received,
                    total_chunks,
                };
            }
        }
    }

    /// Mark a file as downloaded.
    pub fn file_completed(&mut self, hash: &str) {
        if let Some(file) = self.files.iter_mut().find(|f| f.hash == hash) {
            file.status = FileStatus::Downloaded;
        }
    }

    /// Mark a download as failed.
    pub fn download_failed(&mut self, hash: &str, error: String) {
        if let Some(file) = self.files.iter_mut().find(|f| f.hash == hash) {
            file.status = FileStatus::Failed(error);
        }
    }

    /// Add a peer that has a specific file.
    pub fn add_peer_for_file(&mut self, hash: &str, peer_id: String) {
        if let Some(file) = self.files.iter_mut().find(|f| f.hash == hash) {
            if !file.available_from.contains(&peer_id) {
                file.available_from.push(peer_id);
            }
        }
    }

    /// Set an error message to display in the panel.
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Clear any displayed error.
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// Handle a message and return any external action.
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::RequestDownload(hash) => {
                if let Some(file) = self.files.iter_mut().find(|f| f.hash == hash) {
                    file.status = FileStatus::Downloading {
                        chunks_received: 0,
                        total_chunks: 0,
                    };
                }
                Action::Download(hash)
            }
            Message::ShareFile => Action::OpenFilePicker,
        }
    }

    /// Render the files panel.
    pub fn view(&self) -> Element<'_, Message, Theme, Renderer> {
        let title = text("Shared Files").size(16);
        let share_btn = button(text("Share File")).on_press(Message::ShareFile);

        let mut file_list = column![].spacing(8);

        for file in &self.files {
            let size_str = format_size(file.size);
            let header = text(format!("{} ({})", file.name, size_str)).size(14);
            let sender = text(format!("From: {}", file.sender)).size(11);

            let status_widget: Element<'_, Message, Theme, Renderer> = match &file.status {
                FileStatus::Available => {
                    let dl_btn = button(text("Download"))
                        .on_press(Message::RequestDownload(file.hash.clone()));
                    let peers = text(format!("{} peer(s)", file.available_from.len())).size(11);
                    row![dl_btn, peers].spacing(5).into()
                }
                FileStatus::Downloading {
                    chunks_received,
                    total_chunks,
                } => {
                    let progress = if *total_chunks > 0 {
                        *chunks_received as f32 / *total_chunks as f32
                    } else {
                        0.0
                    };
                    let bar = progress_bar(0.0..=1.0, progress);
                    let label =
                        text(format!("{}/{} chunks", chunks_received, total_chunks)).size(11);
                    row![bar, label]
                        .spacing(5)
                        .align_y(Alignment::Center)
                        .into()
                }
                FileStatus::Downloaded => text("Downloaded").size(12).into(),
                FileStatus::Failed(err) => text(format!("Failed: {}", err)).size(12).into(),
            };

            let entry = column![header, sender, status_widget].spacing(3);
            file_list = file_list.push(entry);
        }

        if self.files.is_empty() {
            file_list = file_list.push(text("No files shared yet").size(12));
        }

        let mut content = column![title, share_btn].spacing(10).padding(10);

        if let Some(ref err) = self.error {
            content = content.push(
                text(err)
                    .size(12)
                    .color(iced::Color::from_rgb(1.0, 0.3, 0.3)),
            );
        }

        let content = content.push(scrollable(file_list).height(Length::Fill));

        container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

/// Format file size in human-readable form.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_panel() {
        let panel = FilesPanel::default();
        assert!(panel.files.is_empty());
    }

    #[test]
    fn file_announce_adds_to_list() {
        let mut panel = FilesPanel::default();
        panel.file_announced(
            "abc".to_string(),
            "test.txt".to_string(),
            1024,
            "Alice".to_string(),
        );
        assert_eq!(panel.files.len(), 1);
        assert_eq!(panel.files[0].name, "test.txt");
        assert_eq!(panel.files[0].status, FileStatus::Available);

        // Duplicate announce is ignored
        panel.file_announced(
            "abc".to_string(),
            "test.txt".to_string(),
            1024,
            "Alice".to_string(),
        );
        assert_eq!(panel.files.len(), 1);
    }

    #[test]
    fn download_button_emits_action() {
        let mut panel = FilesPanel::default();
        panel.file_announced(
            "abc".to_string(),
            "test.txt".to_string(),
            1024,
            "Alice".to_string(),
        );

        let action = panel.update(Message::RequestDownload("abc".to_string()));
        assert_eq!(action, Action::Download("abc".to_string()));
    }

    #[test]
    fn progress_updates() {
        let mut panel = FilesPanel::default();
        panel.file_announced(
            "abc".to_string(),
            "test.txt".to_string(),
            1024,
            "Alice".to_string(),
        );

        panel.update_progress("abc", 3, 10);
        assert_eq!(
            panel.files[0].status,
            FileStatus::Downloading {
                chunks_received: 3,
                total_chunks: 10
            }
        );
    }

    #[test]
    fn completed_file_shows_downloaded() {
        let mut panel = FilesPanel::default();
        panel.file_announced(
            "abc".to_string(),
            "test.txt".to_string(),
            1024,
            "Alice".to_string(),
        );

        panel.file_completed("abc");
        assert_eq!(panel.files[0].status, FileStatus::Downloaded);
    }

    #[test]
    fn progress_completes_automatically() {
        let mut panel = FilesPanel::default();
        panel.file_announced(
            "abc".to_string(),
            "test.txt".to_string(),
            1024,
            "Alice".to_string(),
        );

        panel.update_progress("abc", 10, 10);
        assert_eq!(panel.files[0].status, FileStatus::Downloaded);
    }

    #[test]
    fn share_file_action() {
        let mut panel = FilesPanel::default();
        let action = panel.update(Message::ShareFile);
        assert_eq!(action, Action::OpenFilePicker);
    }

    #[test]
    fn format_size_ranges() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1_048_576), "1.0 MB");
        assert_eq!(format_size(1_073_741_824), "1.0 GB");
    }
}
