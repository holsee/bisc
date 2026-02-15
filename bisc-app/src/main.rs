mod app;
pub mod settings;
mod voice;

use std::sync::{Arc, Mutex};

use app::{AppAction, AppMessage, Screen};
use bisc_net::{BiscEndpoint, Channel, ChannelEvent, GossipHandle};
use bisc_protocol::channel::ChannelMessage;
use bisc_protocol::ticket::BiscTicket;
use bisc_protocol::types::EndpointId;
use iced::futures::Stream;
use iroh::protocol::Router;
use settings::Settings;
use tokio::sync::mpsc;
use voice::VoiceState;

use iced::{Element, Subscription};
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

/// Map a `ChannelEvent` to an `AppMessage`.
fn channel_event_to_message(event: ChannelEvent) -> AppMessage {
    match event {
        ChannelEvent::PeerJoined(info) => AppMessage::PeerJoined {
            name: info.display_name,
            id: info.endpoint_id.to_hex(),
            mic_enabled: info.capabilities.audio,
            camera_enabled: info.capabilities.video,
            screen_sharing: info.capabilities.screen_share,
        },
        ChannelEvent::PeerLeft(id) => AppMessage::PeerLeft(id.to_hex()),
        ChannelEvent::PeerConnected(id) => AppMessage::PeerConnected(id.to_hex()),
        ChannelEvent::MediaStateChanged {
            endpoint_id,
            audio_muted,
            video_enabled,
            screen_sharing,
            ..
        } => AppMessage::MediaStateChanged {
            peer_id: endpoint_id.to_hex(),
            audio_muted,
            video_enabled,
            screen_sharing,
        },
        ChannelEvent::FileAnnounced {
            endpoint_id,
            file_hash,
            file_name,
            file_size,
            ..
        } => AppMessage::FileAnnounced {
            sender_id: endpoint_id.to_hex(),
            hash: data_encoding::HEXLOWER.encode(&file_hash),
            name: file_name,
            size: file_size,
        },
    }
}

/// Wrapper around the shared event receiver slot, implementing `Hash`
/// so it can serve as a `Subscription::run_with` identity key.
struct ChannelEventRxSlot(Arc<Mutex<Option<mpsc::UnboundedReceiver<ChannelEvent>>>>);

impl std::hash::Hash for ChannelEventRxSlot {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Fixed identity — there is only one channel event subscription.
        "channel-events".hash(state);
    }
}

/// Build a stream of `AppMessage` values from the shared event receiver slot.
///
/// The stream polls the slot until a receiver appears, then reads events from
/// it. If the receiver is closed (channel left), the stream waits for a new
/// one (user creates/joins another channel).
fn channel_event_stream(slot: &ChannelEventRxSlot) -> impl Stream<Item = AppMessage> {
    let rx_slot = slot.0.clone();
    iced::stream::channel(64, async move |mut output| {
        // Take the receiver out of the shared slot.
        let mut rx = loop {
            {
                let mut guard = rx_slot.lock().unwrap();
                if let Some(rx) = guard.take() {
                    break rx;
                }
            }
            // No receiver yet — poll periodically until one appears.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        };

        loop {
            match rx.recv().await {
                Some(event) => {
                    let msg = channel_event_to_message(event);
                    // If the UI side is gone, just stop.
                    if output.try_send(msg).is_err() {
                        tracing::debug!("channel event subscription output closed");
                        break;
                    }
                }
                None => {
                    tracing::info!("channel event stream ended");
                    // Channel was dropped / shut down. Wait for a new
                    // receiver to appear (user might create/join again).
                    loop {
                        {
                            let mut guard = rx_slot.lock().unwrap();
                            if let Some(new_rx) = guard.take() {
                                rx = new_rx;
                                break;
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        }
    })
}

/// Start a voice pipeline for a newly connected peer.
async fn start_peer_voice(
    voice: &Arc<tokio::sync::Mutex<VoiceState>>,
    net: &Arc<Mutex<Option<Net>>>,
    peer_hex_id: &str,
) -> anyhow::Result<()> {
    let endpoint_id =
        EndpointId::from_hex(peer_hex_id).ok_or_else(|| anyhow::anyhow!("invalid peer hex id"))?;

    // Get the QUIC connection (non-async, using try_read on the connections RwLock)
    let connection = {
        let guard = net.lock().unwrap();
        let n = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("net not initialized"))?;
        let channel = n
            .channel
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no active channel"))?;
        channel
            .peer_quic_connection(&endpoint_id)
            .ok_or_else(|| anyhow::anyhow!("no connection to peer"))?
    };

    voice
        .lock()
        .await
        .add_peer(peer_hex_id.to_string(), connection)
        .await?;

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
    /// Channel event receiver, extracted from Channel on create/join.
    channel_event_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<ChannelEvent>>>>,
    /// Voice call state: audio devices, pipelines, mixing.
    voice: Arc<tokio::sync::Mutex<VoiceState>>,
}

impl Default for BiscApp {
    fn default() -> Self {
        let settings = Settings::load();
        Self {
            app: app::App::default(),
            settings,
            net: Arc::new(Mutex::new(None)),
            channel_event_rx: Arc::new(Mutex::new(None)),
            voice: Arc::new(tokio::sync::Mutex::new(VoiceState::new())),
        }
    }
}

impl BiscApp {
    fn update(&mut self, message: AppMessage) -> iced::Task<AppMessage> {
        // Pre-process voice-relevant events before delegating to app state.
        match &message {
            AppMessage::ChannelCreated(_) | AppMessage::ChannelJoined => {
                // Initialize audio devices when entering a channel
                let voice = Arc::clone(&self.voice);
                tokio::spawn(async move {
                    voice.lock().await.init();
                });
            }
            AppMessage::PeerConnected(hex_id) => {
                // Start a voice pipeline for the newly connected peer
                let voice = Arc::clone(&self.voice);
                let net = Arc::clone(&self.net);
                let peer_id = hex_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = start_peer_voice(&voice, &net, &peer_id).await {
                        tracing::warn!(
                            peer_id = %peer_id,
                            error = %e,
                            "failed to start voice pipeline for peer"
                        );
                    }
                });
            }
            AppMessage::PeerLeft(hex_id) => {
                // Stop the voice pipeline for the departing peer
                let voice = Arc::clone(&self.voice);
                let peer_id = hex_id.clone();
                tokio::spawn(async move {
                    voice.lock().await.remove_peer(&peer_id).await;
                });
            }
            _ => {}
        }

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
                let event_rx_slot = Arc::clone(&self.channel_event_rx);
                iced::Task::perform(
                    async move {
                        ensure_net_initialized(&net).await?;
                        let (endpoint, gossip) = {
                            let guard = net.lock().unwrap();
                            let n = guard.as_ref().unwrap();
                            (n.endpoint.endpoint().clone(), n.gossip.clone())
                        };
                        let (mut channel, ticket) =
                            Channel::create(&endpoint, &gossip, display_name, None).await?;
                        let ticket_str = ticket.to_string();
                        // Extract event receiver before storing channel
                        let rx = channel.take_event_receiver();
                        *event_rx_slot.lock().unwrap() = rx;
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
                let event_rx_slot = Arc::clone(&self.channel_event_rx);
                iced::Task::perform(
                    async move {
                        ensure_net_initialized(&net).await?;
                        let bisc_ticket: BiscTicket = ticket.parse()?;
                        let (endpoint, gossip) = {
                            let guard = net.lock().unwrap();
                            let n = guard.as_ref().unwrap();
                            (n.endpoint.endpoint().clone(), n.gossip.clone())
                        };
                        let mut channel =
                            Channel::join(&endpoint, &gossip, &bisc_ticket, display_name, None)
                                .await?;
                        // Extract event receiver before storing channel
                        let rx = channel.take_event_receiver();
                        *event_rx_slot.lock().unwrap() = rx;
                        net.lock().unwrap().as_mut().unwrap().channel = Some(channel);
                        Ok::<_, anyhow::Error>(())
                    },
                    |result| match result {
                        Ok(()) => AppMessage::ChannelJoined,
                        Err(e) => AppMessage::ChannelError(e.to_string()),
                    },
                )
            }
            AppAction::SetMic(on) => {
                let voice = Arc::clone(&self.voice);
                let net = Arc::clone(&self.net);
                tokio::spawn(async move {
                    voice.lock().await.set_muted(!on);
                    // Broadcast media state change via gossip
                    let our_id = {
                        let guard = net.lock().unwrap();
                        guard
                            .as_ref()
                            .and_then(|n| n.channel.as_ref().map(|c| c.our_endpoint_id()))
                    };
                    if let Some(endpoint_id) = our_id {
                        let guard = net.lock().unwrap();
                        if let Some(ref n) = *guard {
                            if let Some(ref channel) = n.channel {
                                channel.broadcast_message(ChannelMessage::MediaStateUpdate {
                                    endpoint_id,
                                    audio_muted: !on,
                                    video_enabled: false,
                                    screen_sharing: false,
                                    app_audio_sharing: false,
                                });
                            }
                        }
                    }
                });
                iced::Task::none()
            }
            AppAction::LeaveChannel => {
                // Stop all voice pipelines
                let voice = Arc::clone(&self.voice);
                tokio::spawn(async move {
                    voice.lock().await.shutdown().await;
                });
                // Drop the event receiver (ends the subscription)
                *self.channel_event_rx.lock().unwrap() = None;
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
        use iced::widget::{button, column, container, row, text};
        use iced::Length;

        let settings_btn = button(text("Settings")).on_press(AppMessage::OpenSettings);

        match self.app.screen {
            Screen::Channel => {
                let channel_view = self.app.channel_screen.view().map(AppMessage::Channel);
                column![container(settings_btn).padding(5), channel_view,].into()
            }
            Screen::Call => {
                if let Some(call) = &self.app.call_screen {
                    let call_view = call.view().map(AppMessage::Call);
                    let files_view = container(self.app.files_panel.view().map(AppMessage::Files))
                        .width(Length::Fixed(220.0));

                    column![
                        container(settings_btn).padding(5),
                        row![call_view, files_view].height(Length::Fill),
                    ]
                    .into()
                } else {
                    self.app.channel_screen.view().map(AppMessage::Channel)
                }
            }
            Screen::Settings => self.app.settings_screen.view().map(AppMessage::Settings),
        }
    }

    fn subscription(&self) -> Subscription<AppMessage> {
        Subscription::run_with(
            ChannelEventRxSlot(Arc::clone(&self.channel_event_rx)),
            channel_event_stream,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bisc_net::PeerInfo;
    use bisc_protocol::channel::MediaCapabilities;
    use bisc_protocol::types::EndpointId;

    fn init_test_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "debug".into()),
            )
            .with_test_writer()
            .try_init();
    }

    fn dummy_endpoint_id() -> EndpointId {
        EndpointId([
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ])
    }

    #[test]
    fn maps_peer_joined() {
        init_test_tracing();
        let id = dummy_endpoint_id();
        let event = ChannelEvent::PeerJoined(PeerInfo {
            endpoint_id: id.clone(),
            display_name: "Alice".to_string(),
            capabilities: MediaCapabilities {
                audio: true,
                video: true,
                screen_share: false,
            },
            last_heartbeat: std::time::Instant::now(),
        });

        let msg = channel_event_to_message(event);
        match msg {
            AppMessage::PeerJoined {
                name,
                id: msg_id,
                mic_enabled,
                camera_enabled,
                screen_sharing,
            } => {
                assert_eq!(name, "Alice");
                assert_eq!(msg_id, id.to_hex());
                assert!(mic_enabled);
                assert!(camera_enabled);
                assert!(!screen_sharing);
            }
            other => panic!("expected PeerJoined, got {:?}", other),
        }
    }

    #[test]
    fn maps_peer_left() {
        init_test_tracing();
        let id = dummy_endpoint_id();
        let msg = channel_event_to_message(ChannelEvent::PeerLeft(id.clone()));
        match msg {
            AppMessage::PeerLeft(hex) => assert_eq!(hex, id.to_hex()),
            other => panic!("expected PeerLeft, got {:?}", other),
        }
    }

    #[test]
    fn maps_peer_connected() {
        init_test_tracing();
        let id = dummy_endpoint_id();
        let msg = channel_event_to_message(ChannelEvent::PeerConnected(id.clone()));
        match msg {
            AppMessage::PeerConnected(hex) => assert_eq!(hex, id.to_hex()),
            other => panic!("expected PeerConnected, got {:?}", other),
        }
    }

    #[test]
    fn maps_media_state_changed() {
        init_test_tracing();
        let id = dummy_endpoint_id();
        let msg = channel_event_to_message(ChannelEvent::MediaStateChanged {
            endpoint_id: id.clone(),
            audio_muted: true,
            video_enabled: false,
            screen_sharing: true,
            app_audio_sharing: false,
        });
        match msg {
            AppMessage::MediaStateChanged {
                peer_id,
                audio_muted,
                video_enabled,
                screen_sharing,
            } => {
                assert_eq!(peer_id, id.to_hex());
                assert!(audio_muted);
                assert!(!video_enabled);
                assert!(screen_sharing);
            }
            other => panic!("expected MediaStateChanged, got {:?}", other),
        }
    }

    #[test]
    fn maps_file_announced() {
        init_test_tracing();
        let id = dummy_endpoint_id();
        let hash = [42u8; 32];
        let msg = channel_event_to_message(ChannelEvent::FileAnnounced {
            endpoint_id: id.clone(),
            file_hash: hash,
            file_name: "photo.jpg".to_string(),
            file_size: 1024,
            chunk_count: 4,
        });
        match msg {
            AppMessage::FileAnnounced {
                sender_id,
                hash: h,
                name,
                size,
            } => {
                assert_eq!(sender_id, id.to_hex());
                assert_eq!(h, data_encoding::HEXLOWER.encode(&hash));
                assert_eq!(name, "photo.jpg");
                assert_eq!(size, 1024);
            }
            other => panic!("expected FileAnnounced, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn subscription_receives_channel_events() {
        init_test_tracing();
        let (tx, rx) = mpsc::unbounded_channel::<ChannelEvent>();
        let slot: Arc<Mutex<Option<mpsc::UnboundedReceiver<ChannelEvent>>>> =
            Arc::new(Mutex::new(Some(rx)));

        let id = dummy_endpoint_id();
        tx.send(ChannelEvent::PeerJoined(PeerInfo {
            endpoint_id: id,
            display_name: "Test".to_string(),
            capabilities: MediaCapabilities {
                audio: true,
                video: false,
                screen_share: false,
            },
            last_heartbeat: std::time::Instant::now(),
        }))
        .unwrap();

        // Read the first event from the subscription stream.
        // The stream loops forever (waiting for new receivers on close),
        // so we use next() + timeout instead of collect().
        use iced::futures::StreamExt;
        let mut stream = std::pin::pin!(channel_event_stream(&ChannelEventRxSlot(slot)));
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("timed out waiting for event")
            .expect("stream ended unexpectedly");

        match msg {
            AppMessage::PeerJoined { name, .. } => assert_eq!(name, "Test"),
            other => panic!("expected PeerJoined, got {:?}", other),
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
        .subscription(BiscApp::subscription)
        .title("bisc")
        .theme(iced::Theme::Dark)
        .run()
}
