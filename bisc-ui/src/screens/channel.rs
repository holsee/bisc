//! Channel screen: create or join a channel.
//!
//! The initial screen where users enter their display name, create a new channel,
//! or join an existing one by pasting a ticket.

use iced::widget::{button, column, container, row, text, text_input, Column};
use iced::{Alignment, Element, Length, Renderer, Theme};

/// Messages emitted by the channel screen.
#[derive(Debug, Clone)]
pub enum Message {
    /// Display name text input changed.
    DisplayNameChanged(String),
    /// Ticket text input changed.
    TicketChanged(String),
    /// User clicked "Create Channel".
    CreateChannel,
    /// User clicked "Join Channel".
    JoinChannel,
    /// User clicked "Copy Ticket".
    CopyTicket,
}

/// State of the channel screen.
#[derive(Debug, Clone)]
pub struct ChannelScreen {
    /// Current display name.
    pub display_name: String,
    /// Current ticket input text.
    pub ticket_input: String,
    /// Ticket generated after channel creation (None if not yet created).
    pub created_ticket: Option<String>,
    /// Error message to display.
    pub error: Option<String>,
}

impl Default for ChannelScreen {
    fn default() -> Self {
        Self {
            display_name: "user".to_string(),
            ticket_input: String::new(),
            created_ticket: None,
            error: None,
        }
    }
}

/// Result of processing a channel screen message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No external action needed.
    None,
    /// Request to create a new channel with the given display name.
    CreateChannel { display_name: String },
    /// Request to join a channel with the given ticket and display name.
    JoinChannel {
        ticket: String,
        display_name: String,
    },
    /// Request to copy text to clipboard.
    CopyToClipboard(String),
}

impl ChannelScreen {
    /// Create a new channel screen with the given display name.
    pub fn new(display_name: String) -> Self {
        Self {
            display_name,
            ..Default::default()
        }
    }

    /// Set the created ticket (after successful channel creation).
    pub fn set_ticket(&mut self, ticket: String) {
        self.created_ticket = Some(ticket);
        self.error = None;
    }

    /// Set an error message.
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Handle a message and return any external action to perform.
    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::DisplayNameChanged(name) => {
                self.display_name = name;
                Action::None
            }
            Message::TicketChanged(ticket) => {
                self.ticket_input = ticket;
                self.error = None;
                Action::None
            }
            Message::CreateChannel => {
                if self.display_name.trim().is_empty() {
                    self.error = Some("Display name cannot be empty".to_string());
                    return Action::None;
                }
                self.error = None;
                Action::CreateChannel {
                    display_name: self.display_name.clone(),
                }
            }
            Message::JoinChannel => {
                if self.display_name.trim().is_empty() {
                    self.error = Some("Display name cannot be empty".to_string());
                    return Action::None;
                }
                if self.ticket_input.trim().is_empty() {
                    self.error = Some("Please enter a ticket to join".to_string());
                    return Action::None;
                }
                self.error = None;
                Action::JoinChannel {
                    ticket: self.ticket_input.clone(),
                    display_name: self.display_name.clone(),
                }
            }
            Message::CopyTicket => {
                if let Some(ticket) = &self.created_ticket {
                    Action::CopyToClipboard(ticket.clone())
                } else {
                    Action::None
                }
            }
        }
    }

    /// Render the channel screen.
    pub fn view(&self) -> Element<'_, Message, Theme, Renderer> {
        let title = text("bisc").size(32);

        let name_input = text_input("Display name...", &self.display_name)
            .on_input(Message::DisplayNameChanged)
            .padding(10)
            .width(Length::Fixed(300.0));

        let create_btn = button(text("Create Channel")).on_press(Message::CreateChannel);

        let ticket_section: Column<'_, Message, Theme, Renderer> =
            if let Some(ticket) = &self.created_ticket {
                let ticket_display = text(ticket.as_str()).size(12);
                let copy_btn = button(text("Copy Ticket")).on_press(Message::CopyTicket);
                column![ticket_display, copy_btn]
                    .spacing(5)
                    .align_x(Alignment::Center)
            } else {
                column![]
            };

        let separator = text("— or —").size(14);

        let ticket_input = text_input("Paste ticket here...", &self.ticket_input)
            .on_input(Message::TicketChanged)
            .padding(10)
            .width(Length::Fixed(300.0));

        let join_btn = button(text("Join Channel")).on_press(Message::JoinChannel);

        let error_display: Element<'_, Message, Theme, Renderer> = if let Some(err) = &self.error {
            text(err.as_str()).color([1.0, 0.3, 0.3]).into()
        } else {
            text("").into()
        };

        let content = column![
            title,
            name_input,
            create_btn,
            ticket_section,
            separator,
            row![ticket_input, join_btn]
                .spacing(10)
                .align_y(Alignment::Center),
            error_display,
        ]
        .spacing(15)
        .align_x(Alignment::Center)
        .padding(40);

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
        let screen = ChannelScreen::default();
        assert_eq!(screen.display_name, "user");
        assert!(screen.ticket_input.is_empty());
        assert!(screen.created_ticket.is_none());
        assert!(screen.error.is_none());
    }

    #[test]
    fn create_channel_emits_action() {
        let mut screen = ChannelScreen::new("Alice".to_string());
        let action = screen.update(Message::CreateChannel);
        assert_eq!(
            action,
            Action::CreateChannel {
                display_name: "Alice".to_string()
            }
        );
        assert!(screen.error.is_none());
    }

    #[test]
    fn create_channel_empty_name_shows_error() {
        let mut screen = ChannelScreen::new("".to_string());
        let action = screen.update(Message::CreateChannel);
        assert_eq!(action, Action::None);
        assert!(screen.error.is_some());
        assert!(screen.error.as_ref().unwrap().contains("name"));
    }

    #[test]
    fn join_empty_ticket_shows_error() {
        let mut screen = ChannelScreen::new("Bob".to_string());
        screen.ticket_input = "".to_string();
        let action = screen.update(Message::JoinChannel);
        assert_eq!(action, Action::None);
        assert!(screen.error.is_some());
        assert!(screen.error.as_ref().unwrap().contains("ticket"));
    }

    #[test]
    fn join_with_ticket_emits_action() {
        let mut screen = ChannelScreen::new("Bob".to_string());
        screen.ticket_input = "some-ticket-data".to_string();
        let action = screen.update(Message::JoinChannel);
        assert_eq!(
            action,
            Action::JoinChannel {
                ticket: "some-ticket-data".to_string(),
                display_name: "Bob".to_string()
            }
        );
    }

    #[test]
    fn join_empty_name_shows_error() {
        let mut screen = ChannelScreen::new("  ".to_string());
        screen.ticket_input = "ticket".to_string();
        let action = screen.update(Message::JoinChannel);
        assert_eq!(action, Action::None);
        assert!(screen.error.is_some());
    }

    #[test]
    fn set_ticket_clears_error() {
        let mut screen = ChannelScreen::default();
        screen.error = Some("previous error".to_string());
        screen.set_ticket("new-ticket".to_string());
        assert_eq!(screen.created_ticket, Some("new-ticket".to_string()));
        assert!(screen.error.is_none());
    }

    #[test]
    fn copy_ticket_returns_clipboard_action() {
        let mut screen = ChannelScreen::default();
        screen.set_ticket("copy-me".to_string());
        let action = screen.update(Message::CopyTicket);
        assert_eq!(action, Action::CopyToClipboard("copy-me".to_string()));
    }

    #[test]
    fn copy_ticket_no_ticket_returns_none() {
        let mut screen = ChannelScreen::default();
        let action = screen.update(Message::CopyTicket);
        assert_eq!(action, Action::None);
    }

    #[test]
    fn ticket_change_clears_error() {
        let mut screen = ChannelScreen::default();
        screen.error = Some("error".to_string());
        screen.update(Message::TicketChanged("new".to_string()));
        assert!(screen.error.is_none());
    }

    #[test]
    fn display_name_change_updates_state() {
        let mut screen = ChannelScreen::default();
        screen.update(Message::DisplayNameChanged("Charlie".to_string()));
        assert_eq!(screen.display_name, "Charlie");
    }
}
