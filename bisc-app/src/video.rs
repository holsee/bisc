//! Video call integration: manages camera, video pipelines, and frame delivery.
//!
//! When a user enables their camera, frames flow from the camera through the
//! `VideoPipeline` send loop (H.264 encode → fragment → QUIC datagrams).
//! Incoming video from remote peers flows through the recv loop (reassemble →
//! decode → frame buffer). Decoded frames are stored in shared state so the
//! Iced view can render them via `VideoSurface` widgets.
//!
//! Requires the `video-codec` feature for H.264 codec support.
//! Camera capture additionally requires the `video` feature.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bisc_media::video_types::RawFrame;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

#[cfg(feature = "video-codec")]
use bisc_media::video_pipeline::{VideoConfig, VideoPipeline};

use crate::settings::Quality;

/// Map a `Quality` preset to a `VideoConfig` with appropriate resolution/bitrate bounds.
#[cfg(feature = "video-codec")]
pub fn video_config_for_quality(quality: Quality) -> VideoConfig {
    match quality {
        Quality::Low => VideoConfig {
            width: 480,
            height: 360,
            framerate: 24,
            bitrate_bps: 128_000,
            max_bitrate_bps: 256_000,
            min_bitrate_bps: 64_000,
        },
        Quality::Medium => VideoConfig {
            width: 1280,
            height: 720,
            framerate: 30,
            bitrate_bps: 512_000,
            max_bitrate_bps: 1_000_000,
            min_bitrate_bps: 128_000,
        },
        Quality::High => VideoConfig {
            width: 1920,
            height: 1080,
            framerate: 30,
            bitrate_bps: 2_000_000,
            max_bitrate_bps: 4_000_000,
            min_bitrate_bps: 500_000,
        },
    }
}

/// Manages video state: camera, pipelines, and frame delivery.
pub struct VideoState {
    /// Whether camera capture is active.
    camera_on: bool,
    /// Whether video-codec feature is available at runtime.
    #[allow(dead_code)]
    codec_available: bool,
    /// Active video pipelines keyed by peer hex ID.
    #[cfg(feature = "video-codec")]
    pipelines: HashMap<String, VideoPipeline>,
    /// Decoded frame buffers per peer (written by pipelines, read by UI).
    peer_frames: Arc<Mutex<HashMap<String, RawFrame>>>,
    /// Local camera preview frame (written by camera task, read by UI).
    local_frame: Arc<Mutex<Option<RawFrame>>>,
    /// Camera frame sender (camera task writes here, pipeline reads).
    camera_tx: Option<mpsc::UnboundedSender<RawFrame>>,
    /// Camera task handle.
    camera_handle: Option<JoinHandle<()>>,
    /// Frame relay task handles (per-peer: pipeline output → shared state).
    relay_handles: HashMap<String, JoinHandle<()>>,
    /// Current video quality preset (used when creating new pipelines).
    quality: Quality,
}

impl VideoState {
    /// Create a new video state.
    pub fn new() -> Self {
        Self {
            camera_on: false,
            codec_available: cfg!(feature = "video-codec"),
            #[cfg(feature = "video-codec")]
            pipelines: HashMap::new(),
            peer_frames: Arc::new(Mutex::new(HashMap::new())),
            local_frame: Arc::new(Mutex::new(None)),
            camera_tx: None,
            camera_handle: None,
            relay_handles: HashMap::new(),
            quality: Quality::Medium,
        }
    }

    /// Get the shared peer frame buffers (for UI rendering).
    #[allow(dead_code)]
    pub fn peer_frames(&self) -> &Arc<Mutex<HashMap<String, RawFrame>>> {
        &self.peer_frames
    }

    /// Get the local camera preview frame (for UI rendering).
    pub fn local_frame(&self) -> &Arc<Mutex<Option<RawFrame>>> {
        &self.local_frame
    }

    /// Set the video quality preset. Affects newly created pipelines.
    pub fn set_quality(&mut self, quality: Quality) {
        tracing::info!(?quality, "video quality preset changed");
        self.quality = quality;
    }

    /// Start camera capture.
    ///
    /// Returns true if the camera was successfully started.
    pub fn start_camera(&mut self) -> bool {
        if self.camera_on {
            return true;
        }

        let (tx, _rx) = mpsc::unbounded_channel();
        let started = start_camera_capture(&tx, &self.local_frame);

        if started {
            self.camera_tx = Some(tx);
            self.camera_on = true;
            tracing::info!("camera started");

            // Set camera_on on all existing pipelines
            #[cfg(feature = "video-codec")]
            for pipeline in self.pipelines.values() {
                pipeline.set_camera_on(true);
            }
        } else {
            tracing::warn!("failed to start camera");
        }

        started
    }

    /// Stop camera capture.
    pub fn stop_camera(&mut self) {
        if !self.camera_on {
            return;
        }

        // Drop the camera sender to stop the capture
        self.camera_tx = None;
        if let Some(handle) = self.camera_handle.take() {
            handle.abort();
        }
        *self.local_frame.lock().unwrap() = None;
        self.camera_on = false;

        // Set camera_on on all existing pipelines
        #[cfg(feature = "video-codec")]
        for pipeline in self.pipelines.values() {
            pipeline.set_camera_on(false);
        }

        tracing::info!("camera stopped");
    }

    /// Whether the camera is currently on.
    pub fn is_camera_on(&self) -> bool {
        self.camera_on
    }

    /// Add a video pipeline for a newly connected peer.
    #[cfg(feature = "video-codec")]
    pub async fn add_peer(
        &mut self,
        peer_id: String,
        connection: iroh::endpoint::Connection,
    ) -> anyhow::Result<()> {
        if self.pipelines.contains_key(&peer_id) {
            tracing::debug!(peer_id = %peer_id, "video pipeline already exists");
            return Ok(());
        }

        let config = video_config_for_quality(self.quality);
        tracing::debug!(
            peer_id = %peer_id,
            width = config.width,
            height = config.height,
            bitrate = config.bitrate_bps,
            max_bitrate = config.max_bitrate_bps,
            min_bitrate = config.min_bitrate_bps,
            "creating video pipeline with quality-derived config"
        );

        // Create channels for this pipeline
        let (frame_in_tx, frame_in_rx) = mpsc::unbounded_channel();
        let (frame_out_tx, frame_out_rx) = mpsc::unbounded_channel();

        // Create and start the pipeline
        let mut pipeline = VideoPipeline::new(connection, config, frame_in_rx, frame_out_tx);
        pipeline.set_camera_on(self.camera_on);
        pipeline.start().await?;

        tracing::info!(peer_id = %peer_id, "video pipeline started for peer");

        // If we have a camera, fan-out frames to this pipeline too
        // (The camera_tx is the sender; we need a copy of it for each pipeline's input)
        // For now, we won't fan-out camera frames to video pipelines from here
        // because the camera_tx/rx mechanism needs restructuring for multi-peer.
        // Instead, each pipeline gets its own frame_in_tx that the camera task writes to.
        drop(frame_in_tx); // TODO: wire camera fan-out in future

        // Spawn relay task: pipeline output → shared frame buffer
        let peer_frames = Arc::clone(&self.peer_frames);
        let pid = peer_id.clone();
        let relay_handle = tokio::spawn(frame_relay_task(pid, frame_out_rx, peer_frames));
        self.relay_handles.insert(peer_id.clone(), relay_handle);

        self.pipelines.insert(peer_id, pipeline);
        Ok(())
    }

    /// Stub when video-codec is not available.
    #[cfg(not(feature = "video-codec"))]
    pub async fn add_peer(
        &mut self,
        peer_id: String,
        _connection: iroh::endpoint::Connection,
    ) -> anyhow::Result<()> {
        tracing::info!(
            peer_id = %peer_id,
            "video-codec feature not enabled, skipping video pipeline"
        );
        Ok(())
    }

    /// Remove and stop the video pipeline for a peer.
    pub async fn remove_peer(&mut self, peer_id: &str) {
        #[cfg(feature = "video-codec")]
        if let Some(mut pipeline) = self.pipelines.remove(peer_id) {
            pipeline.stop().await;
            tracing::info!(peer_id = %peer_id, "video pipeline stopped");
        }

        // Stop the relay task
        if let Some(handle) = self.relay_handles.remove(peer_id) {
            handle.abort();
        }

        // Remove the frame buffer
        self.peer_frames.lock().unwrap().remove(peer_id);
    }

    /// Shut down all video pipelines and camera.
    pub async fn shutdown(&mut self) {
        self.stop_camera();

        #[cfg(feature = "video-codec")]
        for (peer_id, mut pipeline) in self.pipelines.drain() {
            pipeline.stop().await;
            tracing::debug!(peer_id = %peer_id, "video pipeline stopped on shutdown");
        }

        // Abort all relay tasks
        for (_, handle) in self.relay_handles.drain() {
            handle.abort();
        }

        self.peer_frames.lock().unwrap().clear();
        tracing::info!("video state shut down");
    }
}

/// Relay task: reads decoded frames from a pipeline's output channel
/// and stores the latest frame in shared state for UI rendering.
#[allow(dead_code)]
async fn frame_relay_task(
    peer_id: String,
    mut frame_rx: mpsc::UnboundedReceiver<RawFrame>,
    peer_frames: Arc<Mutex<HashMap<String, RawFrame>>>,
) {
    tracing::debug!(peer_id = %peer_id, "frame relay task started");
    while let Some(frame) = frame_rx.recv().await {
        peer_frames.lock().unwrap().insert(peer_id.clone(), frame);
    }
    tracing::debug!(peer_id = %peer_id, "frame relay task ended");
}

/// Start camera capture if the `video` feature is enabled.
///
/// Returns true if the camera was successfully started.
#[cfg(feature = "video")]
fn start_camera_capture(
    _camera_tx: &mpsc::UnboundedSender<RawFrame>,
    local_frame: &Arc<Mutex<Option<RawFrame>>>,
) -> bool {
    use bisc_media::camera::{Camera, CameraConfig};

    let config = CameraConfig::default();
    match Camera::open(0, &config) {
        Ok(camera) => {
            let local_frame = Arc::clone(local_frame);
            tracing::info!("camera opened successfully");

            // Spawn a camera capture task
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(33)); // ~30fps
                loop {
                    interval.tick().await;
                    match camera.capture_frame() {
                        Ok(frame) => {
                            *local_frame.lock().unwrap() = Some(frame);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "camera frame capture failed");
                        }
                    }
                }
            });

            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "failed to open camera");
            false
        }
    }
}

/// Stub when the `video` feature is not enabled.
#[cfg(not(feature = "video"))]
fn start_camera_capture(
    _camera_tx: &mpsc::UnboundedSender<RawFrame>,
    _local_frame: &Arc<Mutex<Option<RawFrame>>>,
) -> bool {
    tracing::info!("video feature not enabled, camera disabled");
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_state_starts_with_camera_off() {
        let state = VideoState::new();
        assert!(!state.is_camera_on());
        assert!(state.peer_frames.lock().unwrap().is_empty());
        assert!(state.local_frame.lock().unwrap().is_none());
    }

    #[test]
    fn camera_stop_is_idempotent() {
        let mut state = VideoState::new();
        state.stop_camera(); // Should not panic
        assert!(!state.is_camera_on());
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let mut state = VideoState::new();
        state.shutdown().await;
        assert!(!state.is_camera_on());
    }

    #[tokio::test]
    async fn remove_nonexistent_peer_is_noop() {
        let mut state = VideoState::new();
        state.remove_peer("nonexistent").await;
    }

    #[tokio::test]
    async fn frame_relay_stores_latest_frame() {
        let (tx, rx) = mpsc::unbounded_channel();
        let peer_frames = Arc::new(Mutex::new(HashMap::new()));

        let frames_clone = Arc::clone(&peer_frames);
        let handle = tokio::spawn(frame_relay_task("peer-a".to_string(), rx, frames_clone));

        // Send a frame
        let frame = RawFrame {
            data: vec![255; 640 * 480 * 4],
            width: 640,
            height: 480,
            format: bisc_media::video_types::PixelFormat::Rgba,
        };
        tx.send(frame).unwrap();

        // Give relay time to process
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Check frame was stored
        let frames = peer_frames.lock().unwrap();
        assert!(frames.contains_key("peer-a"));
        assert_eq!(frames["peer-a"].width, 640);
        assert_eq!(frames["peer-a"].height, 480);
        drop(frames);

        // Send another frame (should replace)
        let frame2 = RawFrame {
            data: vec![128; 320 * 240 * 4],
            width: 320,
            height: 240,
            format: bisc_media::video_types::PixelFormat::Rgba,
        };
        tx.send(frame2).unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let frames = peer_frames.lock().unwrap();
        assert_eq!(frames["peer-a"].width, 320);
        assert_eq!(frames["peer-a"].height, 240);
        drop(frames);

        // Drop sender to end relay
        drop(tx);
        tokio::time::timeout(std::time::Duration::from_secs(2), handle)
            .await
            .expect("relay should end")
            .expect("relay should not panic");
    }
}
