//! Screen share integration: manages capture, share pipelines, and frame delivery.
//!
//! When a user starts sharing their screen, frames flow from the screen capture
//! through the `SharePipeline` send loop (H.264 encode → fragment → QUIC datagrams).
//! Incoming screen shares from remote peers flow through the recv loop (reassemble →
//! decode → frame buffer). Decoded frames are stored in shared state so the
//! Iced view can render them via `VideoSurface` widgets.
//!
//! Requires the `video-codec` feature for H.264 codec support.
//! Screen capture additionally requires the `screen-capture` feature.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bisc_media::video_types::RawFrame;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[cfg(feature = "video-codec")]
use bisc_media::share_pipeline::{ShareConfig, SharePipeline};

/// Manages screen share state: capture, pipelines, and frame delivery.
pub struct ScreenShareState {
    /// Whether screen capture is active.
    sharing: bool,
    /// Whether screen-capture feature is available at runtime.
    #[allow(dead_code)]
    capture_available: bool,
    /// Active share pipelines keyed by peer hex ID.
    #[cfg(feature = "video-codec")]
    pipelines: HashMap<String, SharePipeline>,
    /// Decoded share frame buffers per peer (written by pipelines, read by UI).
    share_frames: Arc<Mutex<HashMap<String, RawFrame>>>,
    /// Screen capture frame sender (capture task writes here, pipeline reads).
    #[allow(dead_code)]
    capture_tx: Option<mpsc::UnboundedSender<RawFrame>>,
    /// Screen capture task handle.
    capture_handle: Option<JoinHandle<()>>,
    /// Frame relay task handles (per-peer: pipeline output → shared state).
    relay_handles: HashMap<String, JoinHandle<()>>,
}

impl ScreenShareState {
    /// Create a new screen share state.
    pub fn new() -> Self {
        Self {
            sharing: false,
            capture_available: cfg!(feature = "screen-capture"),
            #[cfg(feature = "video-codec")]
            pipelines: HashMap::new(),
            share_frames: Arc::new(Mutex::new(HashMap::new())),
            capture_tx: None,
            capture_handle: None,
            relay_handles: HashMap::new(),
        }
    }

    /// Get the shared screen share frame buffers (for UI rendering).
    pub fn share_frames(&self) -> &Arc<Mutex<HashMap<String, RawFrame>>> {
        &self.share_frames
    }

    /// Start screen capture (primary display).
    ///
    /// Returns true if screen capture was successfully started.
    pub fn start_sharing(&mut self) -> bool {
        if self.sharing {
            return true;
        }

        let (tx, _rx) = mpsc::unbounded_channel();
        let started = start_screen_capture(&tx);

        if started {
            self.capture_tx = Some(tx);
            self.sharing = true;
            tracing::info!("screen sharing started");
        } else {
            tracing::warn!("failed to start screen capture");
        }

        started
    }

    /// Stop screen capture.
    pub fn stop_sharing(&mut self) {
        if !self.sharing {
            return;
        }

        self.capture_tx = None;
        if let Some(handle) = self.capture_handle.take() {
            handle.abort();
        }
        self.sharing = false;

        tracing::info!("screen sharing stopped");
    }

    /// Whether screen sharing is currently active.
    #[allow(dead_code)]
    pub fn is_sharing(&self) -> bool {
        self.sharing
    }

    /// Add a share pipeline for a newly connected peer.
    #[allow(dead_code)]
    #[cfg(feature = "video-codec")]
    pub async fn add_peer(
        &mut self,
        peer_id: String,
        connection: iroh::endpoint::Connection,
        gossip_tx: Option<mpsc::UnboundedSender<bisc_protocol::channel::ChannelMessage>>,
        endpoint_id: bisc_protocol::types::EndpointId,
    ) -> anyhow::Result<()> {
        if self.pipelines.contains_key(&peer_id) {
            tracing::debug!(peer_id = %peer_id, "share pipeline already exists");
            return Ok(());
        }

        let config = ShareConfig::default();

        let (frame_in_tx, frame_in_rx) = mpsc::unbounded_channel();
        let (frame_out_tx, frame_out_rx) = mpsc::unbounded_channel();

        let mut pipeline = SharePipeline::new(
            connection,
            config,
            frame_in_rx,
            frame_out_tx,
            None, // no app audio for now
            gossip_tx,
            endpoint_id,
        );
        pipeline.start().await?;

        tracing::info!(peer_id = %peer_id, "share pipeline started for peer");

        // TODO: wire screen capture fan-out to pipeline's frame_in_tx
        drop(frame_in_tx);

        // Spawn relay task: pipeline output → shared frame buffer
        let share_frames = Arc::clone(&self.share_frames);
        let pid = peer_id.clone();
        let relay_handle = tokio::spawn(share_frame_relay_task(pid, frame_out_rx, share_frames));
        self.relay_handles.insert(peer_id.clone(), relay_handle);

        self.pipelines.insert(peer_id, pipeline);
        Ok(())
    }

    /// Stub when video-codec is not available.
    #[allow(dead_code)]
    #[cfg(not(feature = "video-codec"))]
    pub async fn add_peer(
        &mut self,
        peer_id: String,
        _connection: iroh::endpoint::Connection,
        _gossip_tx: Option<mpsc::UnboundedSender<bisc_protocol::channel::ChannelMessage>>,
        _endpoint_id: bisc_protocol::types::EndpointId,
    ) -> anyhow::Result<()> {
        tracing::info!(
            peer_id = %peer_id,
            "video-codec feature not enabled, skipping share pipeline"
        );
        Ok(())
    }

    /// Remove and stop the share pipeline for a peer.
    pub async fn remove_peer(&mut self, peer_id: &str) {
        #[cfg(feature = "video-codec")]
        if let Some(mut pipeline) = self.pipelines.remove(peer_id) {
            pipeline.stop().await;
            tracing::info!(peer_id = %peer_id, "share pipeline stopped");
        }

        if let Some(handle) = self.relay_handles.remove(peer_id) {
            handle.abort();
        }

        self.share_frames.lock().unwrap().remove(peer_id);
    }

    /// Shut down all share pipelines and capture.
    pub async fn shutdown(&mut self) {
        self.stop_sharing();

        #[cfg(feature = "video-codec")]
        for (peer_id, mut pipeline) in self.pipelines.drain() {
            pipeline.stop().await;
            tracing::debug!(peer_id = %peer_id, "share pipeline stopped on shutdown");
        }

        for (_, handle) in self.relay_handles.drain() {
            handle.abort();
        }

        self.share_frames.lock().unwrap().clear();
        tracing::info!("screen share state shut down");
    }
}

/// Relay task: reads decoded frames from a share pipeline's output channel
/// and stores the latest frame in shared state for UI rendering.
#[allow(dead_code)]
async fn share_frame_relay_task(
    peer_id: String,
    mut frame_rx: mpsc::UnboundedReceiver<RawFrame>,
    share_frames: Arc<Mutex<HashMap<String, RawFrame>>>,
) {
    tracing::debug!(peer_id = %peer_id, "share frame relay task started");
    while let Some(frame) = frame_rx.recv().await {
        share_frames.lock().unwrap().insert(peer_id.clone(), frame);
    }
    tracing::debug!(peer_id = %peer_id, "share frame relay task ended");
}

/// Start screen capture if the `screen-capture` feature is enabled.
///
/// Returns true if capture was successfully started.
#[cfg(feature = "screen-capture")]
fn start_screen_capture(_capture_tx: &mpsc::UnboundedSender<RawFrame>) -> bool {
    use bisc_media::screen_capture;

    if !screen_capture::is_supported() {
        tracing::warn!("screen capture not supported on this platform");
        return false;
    }

    if !screen_capture::has_permission() {
        tracing::info!("requesting screen capture permission");
        if !screen_capture::request_permission() {
            tracing::warn!("screen capture permission denied");
            return false;
        }
    }

    let displays = screen_capture::list_displays();
    if displays.is_empty() {
        tracing::warn!("no displays available for capture");
        return false;
    }

    // Auto-select primary (first) display
    let display = &displays[0];
    tracing::info!(
        display_name = %display.name,
        display_id = display.id,
        width = display.width,
        height = display.height,
        "starting screen capture on primary display"
    );

    match screen_capture::capture_display(display.id, 15) {
        Ok(mut stream) => {
            let tx = _capture_tx.clone();
            tokio::spawn(async move {
                loop {
                    match stream.next_frame().await {
                        Ok(Some(frame)) => {
                            if tx.send(frame).is_err() {
                                tracing::debug!("screen capture output channel closed");
                                break;
                            }
                        }
                        Ok(None) => {
                            tracing::info!("screen capture stream ended");
                            break;
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "screen capture frame failed");
                        }
                    }
                }
            });
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to start screen capture");
            false
        }
    }
}

/// Stub when the `screen-capture` feature is not enabled.
#[cfg(not(feature = "screen-capture"))]
fn start_screen_capture(_capture_tx: &mpsc::UnboundedSender<RawFrame>) -> bool {
    tracing::info!("screen-capture feature not enabled, screen sharing disabled");
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn screen_share_state_starts_inactive() {
        let state = ScreenShareState::new();
        assert!(!state.is_sharing());
        assert!(state.share_frames.lock().unwrap().is_empty());
    }

    #[test]
    fn stop_sharing_is_idempotent() {
        let mut state = ScreenShareState::new();
        state.stop_sharing();
        assert!(!state.is_sharing());
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let mut state = ScreenShareState::new();
        state.shutdown().await;
        assert!(!state.is_sharing());
    }

    #[tokio::test]
    async fn remove_nonexistent_peer_is_noop() {
        let mut state = ScreenShareState::new();
        state.remove_peer("nonexistent").await;
    }

    #[tokio::test]
    async fn share_frame_relay_stores_latest_frame() {
        let (tx, rx) = mpsc::unbounded_channel();
        let share_frames = Arc::new(Mutex::new(HashMap::new()));

        let frames_clone = Arc::clone(&share_frames);
        let handle = tokio::spawn(share_frame_relay_task(
            "peer-a".to_string(),
            rx,
            frames_clone,
        ));

        let frame = RawFrame {
            data: vec![255; 640 * 480 * 4],
            width: 640,
            height: 480,
            format: bisc_media::video_types::PixelFormat::Rgba,
        };
        tx.send(frame).unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let frames = share_frames.lock().unwrap();
        assert!(frames.contains_key("peer-a"));
        assert_eq!(frames["peer-a"].width, 640);
        drop(frames);

        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("relay should end")
            .expect("relay should not panic");
    }
}
