//! Screen share pipeline: capture → encode → fragment → send.
//!
//! Similar to [`VideoPipeline`](crate::video_pipeline) but sources from screen
//! capture instead of a camera. Uses `stream_id=2` for video and `stream_id=3`
//! for scoped app audio.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use bisc_protocol::channel::ChannelMessage;
use bisc_protocol::types::EndpointId;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::fragmentation::{Fragmenter, Reassembler};
use crate::transport::MediaTransport;
use crate::video_types::RawFrame;

/// Stream ID for screen share video.
pub const SCREEN_STREAM_ID: u8 = 2;

/// Stream ID for scoped app audio.
pub const APP_AUDIO_STREAM_ID: u8 = 3;

/// Default max payload size for fragments.
const MAX_FRAGMENT_PAYLOAD: usize = 1200;

/// Check if encoded H.264 data contains an IDR NAL unit.
fn contains_idr_nal(data: &[u8]) -> bool {
    let mut i = 0;
    while i + 3 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            let nal_start = if data[i + 2] == 1 {
                i + 3
            } else if i + 3 < data.len() && data[i + 2] == 0 && data[i + 3] == 1 {
                i + 4
            } else {
                i += 1;
                continue;
            };
            if nal_start < data.len() {
                let nal_type = data[nal_start] & 0x1F;
                if nal_type == 5 {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Metrics for the screen share pipeline.
pub struct SharePipelineMetrics {
    pub frames_captured: AtomicU64,
    pub frames_encoded: AtomicU64,
    pub fragments_sent: AtomicU64,
    pub fragments_received: AtomicU64,
    pub frames_reassembled: AtomicU64,
    pub frames_decoded: AtomicU64,
    pub audio_packets_sent: AtomicU64,
}

impl SharePipelineMetrics {
    fn new() -> Self {
        Self {
            frames_captured: AtomicU64::new(0),
            frames_encoded: AtomicU64::new(0),
            fragments_sent: AtomicU64::new(0),
            fragments_received: AtomicU64::new(0),
            frames_reassembled: AtomicU64::new(0),
            frames_decoded: AtomicU64::new(0),
            audio_packets_sent: AtomicU64::new(0),
        }
    }
}

/// Configuration for the screen share pipeline.
#[derive(Debug, Clone)]
pub struct ShareConfig {
    pub width: u32,
    pub height: u32,
    pub framerate: u32,
    pub bitrate_bps: u32,
    /// Whether to also capture and send app audio.
    pub include_app_audio: bool,
}

impl Default for ShareConfig {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            framerate: 15,
            bitrate_bps: 1_000_000,
            include_app_audio: false,
        }
    }
}

/// Screen share pipeline.
///
/// Sources screen capture frames from a channel, encodes, fragments, and sends
/// them over the transport. Can optionally include scoped app audio on a
/// separate stream.
pub struct SharePipeline {
    config: ShareConfig,
    frame_in_rx: Option<mpsc::UnboundedReceiver<RawFrame>>,
    frame_out_tx: Option<mpsc::UnboundedSender<RawFrame>>,
    audio_in_rx: Option<mpsc::UnboundedReceiver<Vec<f32>>>,
    sharing: Arc<AtomicBool>,
    metrics: Arc<SharePipelineMetrics>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    send_handle: Option<JoinHandle<()>>,
    recv_handle: Option<JoinHandle<()>>,
    audio_send_handle: Option<JoinHandle<()>>,
    connection: iroh::endpoint::Connection,
    /// Channel for broadcasting MediaStateUpdate messages.
    gossip_tx: Option<mpsc::UnboundedSender<ChannelMessage>>,
    endpoint_id: EndpointId,
}

impl SharePipeline {
    /// Create a new share pipeline.
    ///
    /// - `frame_in_rx`: screen capture frames (from ScreenCapture or test injector)
    /// - `frame_out_tx`: decoded frames from remote shares
    /// - `audio_in_rx`: optional scoped app audio
    /// - `gossip_tx`: channel to broadcast MediaStateUpdate messages
    pub fn new(
        connection: iroh::endpoint::Connection,
        config: ShareConfig,
        frame_in_rx: mpsc::UnboundedReceiver<RawFrame>,
        frame_out_tx: mpsc::UnboundedSender<RawFrame>,
        audio_in_rx: Option<mpsc::UnboundedReceiver<Vec<f32>>>,
        gossip_tx: Option<mpsc::UnboundedSender<ChannelMessage>>,
        endpoint_id: EndpointId,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        tracing::info!(
            peer = %connection.remote_id(),
            width = config.width,
            height = config.height,
            framerate = config.framerate,
            include_audio = config.include_app_audio,
            "share pipeline created"
        );

        Self {
            config,
            frame_in_rx: Some(frame_in_rx),
            frame_out_tx: Some(frame_out_tx),
            audio_in_rx,
            sharing: Arc::new(AtomicBool::new(false)),
            metrics: Arc::new(SharePipelineMetrics::new()),
            shutdown_tx,
            shutdown_rx,
            send_handle: None,
            recv_handle: None,
            audio_send_handle: None,
            connection,
            gossip_tx,
            endpoint_id,
        }
    }

    /// Start the share pipeline and broadcast MediaStateUpdate.
    pub async fn start(&mut self) -> Result<()> {
        let frame_in_rx = self
            .frame_in_rx
            .take()
            .context("pipeline already started")?;
        let frame_out_tx = self
            .frame_out_tx
            .take()
            .context("pipeline already started")?;

        self.sharing.store(true, Ordering::Relaxed);

        let send_handle = tokio::spawn(send_loop(
            self.connection.clone(),
            self.config.clone(),
            frame_in_rx,
            Arc::clone(&self.sharing),
            Arc::clone(&self.metrics),
            self.shutdown_rx.clone(),
        ));

        let recv_handle = tokio::spawn(recv_loop(
            self.connection.clone(),
            frame_out_tx,
            Arc::clone(&self.metrics),
            self.shutdown_rx.clone(),
        ));

        self.send_handle = Some(send_handle);
        self.recv_handle = Some(recv_handle);

        // Start audio send if configured
        if self.config.include_app_audio {
            if let Some(audio_rx) = self.audio_in_rx.take() {
                let audio_handle = tokio::spawn(audio_send_loop(
                    self.connection.clone(),
                    audio_rx,
                    Arc::clone(&self.metrics),
                    self.shutdown_rx.clone(),
                ));
                self.audio_send_handle = Some(audio_handle);
            }
        }

        // Broadcast share started
        self.broadcast_state(true);

        tracing::info!("share pipeline started");
        Ok(())
    }

    /// Stop the share pipeline and broadcast MediaStateUpdate.
    pub async fn stop(&mut self) {
        tracing::info!("stopping share pipeline");
        self.sharing.store(false, Ordering::Relaxed);
        let _ = self.shutdown_tx.send(true);

        if let Some(h) = self.send_handle.take() {
            let _ = h.await;
        }
        if let Some(h) = self.recv_handle.take() {
            let _ = h.await;
        }
        if let Some(h) = self.audio_send_handle.take() {
            let _ = h.await;
        }

        // Broadcast share stopped
        self.broadcast_state(false);

        tracing::info!("share pipeline stopped");
    }

    /// Whether the pipeline is actively sharing.
    pub fn is_sharing(&self) -> bool {
        self.sharing.load(Ordering::Relaxed)
    }

    /// Get the pipeline metrics.
    pub fn metrics(&self) -> &Arc<SharePipelineMetrics> {
        &self.metrics
    }

    /// Broadcast a MediaStateUpdate via gossip.
    fn broadcast_state(&self, screen_sharing: bool) {
        if let Some(tx) = &self.gossip_tx {
            let msg = ChannelMessage::MediaStateUpdate {
                endpoint_id: self.endpoint_id,
                audio_muted: false,
                video_enabled: true,
                screen_sharing,
                app_audio_sharing: screen_sharing && self.config.include_app_audio,
            };
            let _ = tx.send(msg);
            tracing::debug!(screen_sharing, "broadcast MediaStateUpdate");
        }
    }
}

/// Send loop for screen share video.
async fn send_loop(
    connection: iroh::endpoint::Connection,
    config: ShareConfig,
    mut frame_in_rx: mpsc::UnboundedReceiver<RawFrame>,
    sharing: Arc<AtomicBool>,
    metrics: Arc<SharePipelineMetrics>,
    mut shutdown: watch::Receiver<bool>,
) {
    use crate::video_codec::VideoEncoder;

    let mut encoder = match VideoEncoder::new(
        config.width,
        config.height,
        config.framerate,
        config.bitrate_bps,
    ) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "failed to create screen share encoder");
            return;
        }
    };
    let mut fragmenter = Fragmenter::new();
    let mut transport = MediaTransport::new(connection);
    let mut timestamp: u32 = 0;
    let ts_increment = 90_000 / config.framerate;

    tracing::debug!("screen share send loop started");

    loop {
        tokio::select! {
            biased;
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    tracing::debug!("screen share send loop shutdown");
                    return;
                }
            }
            frame = frame_in_rx.recv() => {
                let Some(frame) = frame else {
                    tracing::debug!("screen share frame input closed");
                    return;
                };

                metrics.frames_captured.fetch_add(1, Ordering::Relaxed);

                if !sharing.load(Ordering::Relaxed) {
                    timestamp = timestamp.wrapping_add(ts_increment);
                    continue;
                }

                match encoder.encode(&frame) {
                    Ok(nal_units) => {
                        metrics.frames_encoded.fetch_add(1, Ordering::Relaxed);

                        let encoded: Vec<u8> = nal_units.into_iter().flatten().collect();
                        let is_keyframe = contains_idr_nal(&encoded);

                        let fragments = fragmenter.fragment(
                            SCREEN_STREAM_ID,
                            timestamp,
                            is_keyframe,
                            &encoded,
                            MAX_FRAGMENT_PAYLOAD,
                        );

                        for mut frag in fragments {
                            if let Err(e) = transport.send_media_packet(&mut frag) {
                                tracing::warn!(error = %e, "failed to send screen share fragment");
                                return;
                            }
                            metrics.fragments_sent.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "screen share encode failed");
                    }
                }

                timestamp = timestamp.wrapping_add(ts_increment);
            }
        }
    }
}

/// Receive loop for screen share video.
async fn recv_loop(
    connection: iroh::endpoint::Connection,
    frame_out_tx: mpsc::UnboundedSender<RawFrame>,
    metrics: Arc<SharePipelineMetrics>,
    mut shutdown: watch::Receiver<bool>,
) {
    use crate::video_codec::VideoDecoder;

    let mut decoder = match VideoDecoder::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "failed to create screen share decoder");
            return;
        }
    };
    let mut transport = MediaTransport::new(connection);
    let mut reassembler = Reassembler::new();

    tracing::debug!("screen share receive loop started");

    loop {
        tokio::select! {
            biased;
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    tracing::debug!("screen share receive loop shutdown");
                    return;
                }
            }
            result = transport.recv_media_packet() => {
                match result {
                    Ok(packet) => {
                        if packet.stream_id != SCREEN_STREAM_ID {
                            continue;
                        }

                        metrics.fragments_received.fetch_add(1, Ordering::Relaxed);

                        if let Some(reassembled) = reassembler.push(packet) {
                            metrics.frames_reassembled.fetch_add(1, Ordering::Relaxed);

                            match decoder.decode(&reassembled.data) {
                                Ok(Some(frame)) => {
                                    metrics.frames_decoded.fetch_add(1, Ordering::Relaxed);
                                    if frame_out_tx.send(frame).is_err() {
                                        tracing::debug!("screen share frame output closed");
                                        return;
                                    }
                                }
                                Ok(None) => {}
                                Err(e) => {
                                    tracing::warn!(error = %e, "screen share decode failed");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "screen share transport recv ended");
                        return;
                    }
                }
            }
        }
    }
}

/// Send loop for scoped app audio.
async fn audio_send_loop(
    connection: iroh::endpoint::Connection,
    mut audio_in_rx: mpsc::UnboundedReceiver<Vec<f32>>,
    metrics: Arc<SharePipelineMetrics>,
    mut shutdown: watch::Receiver<bool>,
) {
    use crate::opus_codec::{OpusEncoder, SAMPLES_PER_FRAME};
    use bisc_protocol::media::MediaPacket;

    let mut encoder = match OpusEncoder::new(1, 128_000) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "failed to create app audio encoder");
            return;
        }
    };
    let mut transport = MediaTransport::new(connection);
    let mut timestamp: u32 = 0;

    tracing::debug!("app audio send loop started");

    loop {
        tokio::select! {
            biased;
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    tracing::debug!("app audio send loop shutdown");
                    return;
                }
            }
            samples = audio_in_rx.recv() => {
                let Some(samples) = samples else {
                    tracing::debug!("app audio input closed");
                    return;
                };

                match encoder.encode(&samples) {
                    Ok(encoded) => {
                        let mut packet = MediaPacket {
                            stream_id: APP_AUDIO_STREAM_ID,
                            sequence: 0,
                            timestamp,
                            fragment_index: 0,
                            fragment_count: 1,
                            is_keyframe: false,
                            payload: encoded,
                        };

                        if let Err(e) = transport.send_media_packet(&mut packet) {
                            tracing::warn!(error = %e, "failed to send app audio packet");
                            return;
                        }
                        metrics.audio_packets_sent.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "app audio encode failed");
                    }
                }

                timestamp = timestamp.wrapping_add(SAMPLES_PER_FRAME as u32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ShareConfig::default();
        assert_eq!(config.width, 1920);
        assert_eq!(config.height, 1080);
        assert_eq!(config.framerate, 15);
        assert!(!config.include_app_audio);
    }

    #[test]
    fn stream_ids_distinct() {
        assert_ne!(SCREEN_STREAM_ID, 0); // not voice
        assert_ne!(SCREEN_STREAM_ID, 1); // not camera
        assert_ne!(APP_AUDIO_STREAM_ID, 0);
        assert_ne!(APP_AUDIO_STREAM_ID, 1);
        assert_ne!(SCREEN_STREAM_ID, APP_AUDIO_STREAM_ID);
    }

    #[test]
    fn metrics_increment() {
        let m = SharePipelineMetrics::new();
        m.frames_captured.fetch_add(1, Ordering::Relaxed);
        m.fragments_sent.fetch_add(5, Ordering::Relaxed);
        m.audio_packets_sent.fetch_add(3, Ordering::Relaxed);

        assert_eq!(m.frames_captured.load(Ordering::Relaxed), 1);
        assert_eq!(m.fragments_sent.load(Ordering::Relaxed), 5);
        assert_eq!(m.audio_packets_sent.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn media_state_update_message() {
        let msg = ChannelMessage::MediaStateUpdate {
            endpoint_id: EndpointId([0; 32]),
            audio_muted: false,
            video_enabled: true,
            screen_sharing: true,
            app_audio_sharing: false,
        };

        match msg {
            ChannelMessage::MediaStateUpdate {
                screen_sharing,
                app_audio_sharing,
                ..
            } => {
                assert!(screen_sharing);
                assert!(!app_audio_sharing);
            }
            _ => panic!("wrong message type"),
        }
    }
}
