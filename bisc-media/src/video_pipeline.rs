//! End-to-end video call pipeline: capture → encode → fragment → send → receive
//! → reassemble → decode → deliver.
//!
//! `VideoPipeline` orchestrates the full video path using channels for frame I/O,
//! allowing both real camera input and synthetic test frames.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::fragmentation::{Fragmenter, Reassembler};
use crate::transport::MediaTransport;
use crate::video_codec::{VideoDecoder, VideoEncoder};
use crate::video_types::RawFrame;

/// Check if encoded H.264 data contains an IDR (keyframe) NAL unit.
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

/// Stream ID used for video (audio uses stream 0).
const VIDEO_STREAM_ID: u8 = 1;

/// Default max payload size for fragments (fits comfortably in a QUIC datagram).
const MAX_FRAGMENT_PAYLOAD: usize = 1200;

/// Commands that can be sent to the video send loop.
#[derive(Debug)]
pub enum VideoCommand {
    /// Force the encoder to produce a keyframe.
    ForceKeyframe,
    /// Change the encoder's target bitrate (bps).
    SetBitrate(u32),
}

/// Metrics exposed for observability and test assertions.
pub struct VideoPipelineMetrics {
    pub frames_captured: AtomicU64,
    pub frames_encoded: AtomicU64,
    pub fragments_sent: AtomicU64,
    pub fragments_received: AtomicU64,
    pub frames_reassembled: AtomicU64,
    pub frames_decoded: AtomicU64,
    pub keyframes_forced: AtomicU64,
}

impl VideoPipelineMetrics {
    fn new() -> Self {
        Self {
            frames_captured: AtomicU64::new(0),
            frames_encoded: AtomicU64::new(0),
            fragments_sent: AtomicU64::new(0),
            fragments_received: AtomicU64::new(0),
            frames_reassembled: AtomicU64::new(0),
            frames_decoded: AtomicU64::new(0),
            keyframes_forced: AtomicU64::new(0),
        }
    }
}

/// Configuration for the video pipeline.
#[derive(Debug, Clone)]
pub struct VideoConfig {
    pub width: u32,
    pub height: u32,
    pub framerate: u32,
    pub bitrate_bps: u32,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            framerate: 30,
            bitrate_bps: 500_000,
        }
    }
}

/// End-to-end video pipeline orchestrating send and receive loops.
pub struct VideoPipeline {
    config: VideoConfig,
    frame_in_rx: Option<mpsc::UnboundedReceiver<RawFrame>>,
    frame_out_tx: Option<mpsc::UnboundedSender<RawFrame>>,
    camera_on: Arc<AtomicBool>,
    command_tx: mpsc::UnboundedSender<VideoCommand>,
    metrics: Arc<VideoPipelineMetrics>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    send_handle: Option<JoinHandle<()>>,
    recv_handle: Option<JoinHandle<()>>,
    connection: iroh::endpoint::Connection,
}

impl VideoPipeline {
    /// Create a pipeline from a QUIC connection and frame channels.
    pub fn new(
        connection: iroh::endpoint::Connection,
        config: VideoConfig,
        frame_in_rx: mpsc::UnboundedReceiver<RawFrame>,
        frame_out_tx: mpsc::UnboundedSender<RawFrame>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (command_tx, _command_rx) = mpsc::unbounded_channel();

        tracing::info!(
            peer = %connection.remote_id(),
            width = config.width,
            height = config.height,
            framerate = config.framerate,
            bitrate = config.bitrate_bps,
            "video pipeline created"
        );

        Self {
            config,
            frame_in_rx: Some(frame_in_rx),
            frame_out_tx: Some(frame_out_tx),
            camera_on: Arc::new(AtomicBool::new(true)),
            command_tx,
            metrics: Arc::new(VideoPipelineMetrics::new()),
            shutdown_tx,
            shutdown_rx,
            send_handle: None,
            recv_handle: None,
            connection,
        }
    }

    /// Spawn the send and receive background tasks.
    pub async fn start(&mut self) -> Result<()> {
        let frame_in_rx = self
            .frame_in_rx
            .take()
            .context("pipeline already started (frame_in_rx consumed)")?;
        let frame_out_tx = self
            .frame_out_tx
            .take()
            .context("pipeline already started (frame_out_tx consumed)")?;

        // Create a fresh command channel for the send loop
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        self.command_tx = command_tx;

        let send_handle = tokio::spawn(send_loop(
            self.connection.clone(),
            self.config.clone(),
            frame_in_rx,
            command_rx,
            Arc::clone(&self.camera_on),
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

        tracing::info!("video pipeline started");
        Ok(())
    }

    /// Signal shutdown and await completion of background tasks.
    pub async fn stop(&mut self) {
        tracing::info!("stopping video pipeline");
        let _ = self.shutdown_tx.send(true);

        if let Some(h) = self.send_handle.take() {
            let _ = h.await;
        }
        if let Some(h) = self.recv_handle.take() {
            let _ = h.await;
        }
        tracing::info!("video pipeline stopped");
    }

    /// Toggle camera on/off. When off, the send loop drops incoming frames.
    pub fn set_camera_on(&self, on: bool) {
        self.camera_on.store(on, Ordering::Relaxed);
        tracing::debug!(camera_on = on, "camera state changed");
    }

    /// Check whether the camera is currently enabled.
    pub fn is_camera_on(&self) -> bool {
        self.camera_on.load(Ordering::Relaxed)
    }

    /// Request the encoder to produce a keyframe on the next frame.
    pub fn request_keyframe(&self) {
        let _ = self.command_tx.send(VideoCommand::ForceKeyframe);
    }

    /// Change the encoder's target bitrate.
    pub fn set_bitrate(&self, bps: u32) {
        let _ = self.command_tx.send(VideoCommand::SetBitrate(bps));
    }

    /// Get the pipeline metrics.
    pub fn metrics(&self) -> &Arc<VideoPipelineMetrics> {
        &self.metrics
    }
}

/// Send loop: read frames → encode → fragment → send.
async fn send_loop(
    connection: iroh::endpoint::Connection,
    config: VideoConfig,
    mut frame_in_rx: mpsc::UnboundedReceiver<RawFrame>,
    mut command_rx: mpsc::UnboundedReceiver<VideoCommand>,
    camera_on: Arc<AtomicBool>,
    metrics: Arc<VideoPipelineMetrics>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut encoder = match VideoEncoder::new(
        config.width,
        config.height,
        config.framerate,
        config.bitrate_bps,
    ) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "failed to create video encoder");
            return;
        }
    };
    let mut fragmenter = Fragmenter::new();
    let mut transport = MediaTransport::new(connection);
    let mut timestamp: u32 = 0;
    let ts_increment = 90_000 / config.framerate;

    tracing::debug!("video send loop started");

    loop {
        tokio::select! {
            biased;
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    tracing::debug!("video send loop shutdown");
                    return;
                }
            }
            cmd = command_rx.recv() => {
                match cmd {
                    Some(VideoCommand::ForceKeyframe) => {
                        encoder.force_keyframe();
                        metrics.keyframes_forced.fetch_add(1, Ordering::Relaxed);
                        tracing::debug!("keyframe requested");
                    }
                    Some(VideoCommand::SetBitrate(bps)) => {
                        encoder.set_bitrate(bps);
                        tracing::debug!(bps, "bitrate changed");
                    }
                    None => {
                        tracing::debug!("command channel closed");
                        return;
                    }
                }
            }
            frame = frame_in_rx.recv() => {
                let Some(frame) = frame else {
                    tracing::debug!("frame input channel closed");
                    return;
                };

                metrics.frames_captured.fetch_add(1, Ordering::Relaxed);

                if !camera_on.load(Ordering::Relaxed) {
                    timestamp = timestamp.wrapping_add(ts_increment);
                    continue;
                }

                match encoder.encode(&frame) {
                    Ok(nal_units) => {
                        metrics.frames_encoded.fetch_add(1, Ordering::Relaxed);

                        let encoded: Vec<u8> = nal_units.into_iter().flatten().collect();
                        let is_keyframe = contains_idr_nal(&encoded);

                        let fragments = fragmenter.fragment(
                            VIDEO_STREAM_ID,
                            timestamp,
                            is_keyframe,
                            &encoded,
                            MAX_FRAGMENT_PAYLOAD,
                        );

                        for mut frag in fragments {
                            if let Err(e) = transport.send_media_packet(&mut frag) {
                                tracing::warn!(error = %e, "failed to send video fragment");
                                return;
                            }
                            metrics.fragments_sent.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "video encode failed, skipping frame");
                    }
                }

                timestamp = timestamp.wrapping_add(ts_increment);
            }
        }
    }
}

/// Receive loop: recv fragments → reassemble → decode → deliver.
async fn recv_loop(
    connection: iroh::endpoint::Connection,
    frame_out_tx: mpsc::UnboundedSender<RawFrame>,
    metrics: Arc<VideoPipelineMetrics>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut decoder = match VideoDecoder::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "failed to create video decoder");
            return;
        }
    };
    let mut transport = MediaTransport::new(connection);
    let mut reassembler = Reassembler::new();

    tracing::debug!("video receive loop started");

    loop {
        tokio::select! {
            biased;
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    tracing::debug!("video receive loop shutdown");
                    return;
                }
            }
            result = transport.recv_media_packet() => {
                match result {
                    Ok(packet) => {
                        if packet.stream_id != VIDEO_STREAM_ID {
                            continue;
                        }

                        metrics.fragments_received.fetch_add(1, Ordering::Relaxed);

                        if let Some(reassembled) = reassembler.push(packet) {
                            metrics.frames_reassembled.fetch_add(1, Ordering::Relaxed);

                            match decoder.decode(&reassembled.data) {
                                Ok(Some(frame)) => {
                                    metrics.frames_decoded.fetch_add(1, Ordering::Relaxed);
                                    if frame_out_tx.send(frame).is_err() {
                                        tracing::debug!("frame output channel closed");
                                        return;
                                    }
                                }
                                Ok(None) => {
                                    tracing::trace!(
                                        timestamp = reassembled.timestamp,
                                        "decoder produced no output for frame"
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!(error = %e, "video decode failed");
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "video transport recv ended");
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = VideoConfig::default();
        assert_eq!(config.width, 640);
        assert_eq!(config.height, 480);
        assert_eq!(config.framerate, 30);
        assert_eq!(config.bitrate_bps, 500_000);
    }

    #[test]
    fn metrics_increment() {
        let m = VideoPipelineMetrics::new();
        m.frames_captured.fetch_add(1, Ordering::Relaxed);
        m.frames_encoded.fetch_add(2, Ordering::Relaxed);
        m.fragments_sent.fetch_add(10, Ordering::Relaxed);
        m.fragments_received.fetch_add(9, Ordering::Relaxed);
        m.frames_reassembled.fetch_add(2, Ordering::Relaxed);
        m.frames_decoded.fetch_add(2, Ordering::Relaxed);
        m.keyframes_forced.fetch_add(1, Ordering::Relaxed);

        assert_eq!(m.frames_captured.load(Ordering::Relaxed), 1);
        assert_eq!(m.frames_encoded.load(Ordering::Relaxed), 2);
        assert_eq!(m.fragments_sent.load(Ordering::Relaxed), 10);
        assert_eq!(m.fragments_received.load(Ordering::Relaxed), 9);
        assert_eq!(m.frames_reassembled.load(Ordering::Relaxed), 2);
        assert_eq!(m.frames_decoded.load(Ordering::Relaxed), 2);
        assert_eq!(m.keyframes_forced.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn camera_toggle() {
        let camera_on = AtomicBool::new(true);
        assert!(camera_on.load(Ordering::Relaxed));
        camera_on.store(false, Ordering::Relaxed);
        assert!(!camera_on.load(Ordering::Relaxed));
        camera_on.store(true, Ordering::Relaxed);
        assert!(camera_on.load(Ordering::Relaxed));
    }

    #[test]
    fn timestamp_increment_90khz() {
        let config = VideoConfig {
            framerate: 30,
            ..VideoConfig::default()
        };
        let ts_inc = 90_000 / config.framerate;
        assert_eq!(ts_inc, 3000);

        let ts_inc_15 = 90_000 / 15;
        assert_eq!(ts_inc_15, 6000);
    }

    #[test]
    fn contains_idr_nal_detects_keyframe() {
        // Annex B start code + NAL type 5 (IDR)
        let data = vec![0x00, 0x00, 0x00, 0x01, 0x65, 0xFF, 0xFF];
        assert!(contains_idr_nal(&data));

        // NAL type 1 (non-IDR slice)
        let data = vec![0x00, 0x00, 0x00, 0x01, 0x41, 0xFF, 0xFF];
        assert!(!contains_idr_nal(&data));

        // 3-byte start code + IDR
        let data = vec![0x00, 0x00, 0x01, 0x65, 0xFF];
        assert!(contains_idr_nal(&data));

        // Empty
        assert!(!contains_idr_nal(&[]));
    }
}
