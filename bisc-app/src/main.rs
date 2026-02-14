mod app;
pub mod settings;

use std::sync::{Arc, Mutex};

use app::{AppAction, AppMessage, Screen};
use bisc_net::{BiscEndpoint, Channel, GossipHandle};
use bisc_protocol::ticket::BiscTicket;
use iroh::protocol::Router;
use settings::Settings;

use iced::Element;
use tracing_subscriber::EnvFilter;

/// Networking state shared between the app and async tasks.
struct Net {
    endpoint: BiscEndpoint,
    gossip: GossipHandle,
    _router: Router,
    channel: Option<Channel>,
}

/// Initialize the networking layer if not yet created.
async fn ensure_net_initialized(net: &Arc<Mutex<Option<Net>>>) -> anyhow::Result<()> {
    let needs_init = net.lock().unwrap().is_none();
    if needs_init {
        let ep = BiscEndpoint::new().await?;
        let (gossip, router) = GossipHandle::new(ep.endpoint());
        *net.lock().unwrap() = Some(Net {
            endpoint: ep,
            gossip,
            _router: router,
            channel: None,
        });
    }
    Ok(())
}

/// Top-level Iced application wrapper.
///
/// Bridges the `App` state machine to the Iced runtime by converting
/// `AppAction` returns into `iced::Task` effects.
struct BiscApp {
    app: app::App,
    settings: Settings,
    net: Arc<Mutex<Option<Net>>>,
}

impl Default for BiscApp {
    fn default() -> Self {
        let settings = Settings::load();
        Self {
            app: app::App::default(),
            settings,
            net: Arc::new(Mutex::new(None)),
        }
    }
}

impl BiscApp {
    fn update(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        let action = self.app.update(message);
        match action {
            AppAction::None => iced::Task::none(),
            AppAction::CopyToClipboard(text) => iced::clipboard::write(text),
            AppAction::SaveSettings => {
                self.settings.display_name = self.app.settings_screen.display_name.clone();
                if let Err(e) = self.settings.save() {
                    tracing::error!(error = %e, "failed to save settings");
                }
                iced::Task::none()
            }
            AppAction::CreateChannel(display_name) => {
                let net = Arc::clone(&self.net);
                iced::Task::perform(
                    async move {
                        ensure_net_initialized(&net).await?;
                        let (endpoint, gossip) = {
                            let guard = net.lock().unwrap();
                            let n = guard.as_ref().unwrap();
                            (n.endpoint.endpoint().clone(), n.gossip.clone())
                        };
                        let (channel, ticket) =
                            Channel::create(&endpoint, &gossip, display_name, None).await?;
                        let ticket_str = ticket.to_string();
                        net.lock().unwrap().as_mut().unwrap().channel = Some(channel);
                        Ok::<_, anyhow::Error>(ticket_str)
                    },
                    |result| match result {
                        Ok(ticket) => AppMessage::ChannelCreated(ticket),
                        Err(e) => AppMessage::ChannelError(e.to_string()),
                    },
                )
            }
            AppAction::JoinChannel {
                ticket,
                display_name,
            } => {
                let net = Arc::clone(&self.net);
                iced::Task::perform(
                    async move {
                        ensure_net_initialized(&net).await?;
                        let bisc_ticket: BiscTicket = ticket.parse()?;
                        let (endpoint, gossip) = {
                            let guard = net.lock().unwrap();
                            let n = guard.as_ref().unwrap();
                            (n.endpoint.endpoint().clone(), n.gossip.clone())
                        };
                        let channel =
                            Channel::join(&endpoint, &gossip, &bisc_ticket, display_name, None)
                                .await?;
                        net.lock().unwrap().as_mut().unwrap().channel = Some(channel);
                        Ok::<_, anyhow::Error>(())
                    },
                    |result| match result {
                        Ok(()) => AppMessage::ChannelJoined,
                        Err(e) => AppMessage::ChannelError(e.to_string()),
                    },
                )
            }
            AppAction::LeaveChannel => {
                if let Some(ref mut n) = *self.net.lock().unwrap() {
                    if let Some(channel) = n.channel.take() {
                        channel.leave();
                    }
                }
                iced::Task::none()
            }
            other => {
                tracing::info!(action = ?other, "action not yet wired");
                iced::Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, AppMessage> {
        match self.app.screen {
            Screen::Channel => self.app.channel_screen.view().map(AppMessage::Channel),
            Screen::Call => {
                if let Some(call) = &self.app.call_screen {
                    call.view().map(AppMessage::Call)
                } else {
                    self.app.channel_screen.view().map(AppMessage::Channel)
                }
            }
            Screen::Settings => self.app.settings_screen.view().map(AppMessage::Settings),
        }
    }
}

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    tracing::info!("bisc starting");

    iced::application(BiscApp::default, BiscApp::update, BiscApp::view)
        .title("bisc")
        .theme(iced::Theme::Dark)
        .run()
}
