//! Video call integration: manages camera, video pipelines, and frame delivery.
//!
//! When a user enables their camera, frames flow from the camera through the
//! fan-out mechanism to all active `VideoPipeline` send loops (H.264 encode →
//! fragment → QUIC datagrams). Incoming video from remote peers flows through
//! the recv loop (reassemble → decode → frame buffer). Decoded frames are stored
//! in shared state so the Iced view can render them via `VideoSurface` widgets.
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

/// Shared fan-out registry: maps peer IDs to their pipeline frame input senders.
/// The camera capture thread writes frames here, and pipelines receive them.
type FrameFanout = Arc<Mutex<HashMap<String, mpsc::UnboundedSender<RawFrame>>>>;

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
    /// Fan-out registry: camera thread sends frames to all registered pipeline inputs.
    frame_fanout: FrameFanout,
    /// Camera task handle (std::thread, not tokio).
    camera_stop: Option<Arc<std::sync::atomic::AtomicBool>>,
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
            frame_fanout: Arc::new(Mutex::new(HashMap::new())),
            camera_stop: None,
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

        let stop_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let started = start_camera_capture(&self.local_frame, &self.frame_fanout, &stop_flag);

        if started {
            self.camera_stop = Some(stop_flag);
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

        // Signal the camera thread to stop
        if let Some(stop) = self.camera_stop.take() {
            stop.store(true, std::sync::atomic::Ordering::Relaxed);
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

        // Register this pipeline's frame input with the fan-out so camera frames are delivered
        self.frame_fanout
            .lock()
            .unwrap()
            .insert(peer_id.clone(), frame_in_tx);
        tracing::debug!(peer_id = %peer_id, "registered pipeline with camera fan-out");

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
        // Remove from fan-out first so no more frames are sent
        self.frame_fanout.lock().unwrap().remove(peer_id);

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

        // Clear the fan-out
        self.frame_fanout.lock().unwrap().clear();

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
/// The camera thread captures frames and:
/// 1. Stores the latest frame in `local_frame` for local preview
/// 2. Fans out each frame to all registered pipeline inputs via `frame_fanout`
///
/// Returns true if the camera was successfully started.
#[cfg(feature = "video")]
fn start_camera_capture(
    local_frame: &Arc<Mutex<Option<RawFrame>>>,
    frame_fanout: &FrameFanout,
    stop_flag: &Arc<std::sync::atomic::AtomicBool>,
) -> bool {
    use bisc_media::camera::{Camera, CameraConfig};

    let local_frame = Arc::clone(local_frame);
    let fanout = Arc::clone(frame_fanout);
    let stop = Arc::clone(stop_flag);

    // Camera types (nokhwa) are not Send, so open and use on a dedicated thread
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let config = CameraConfig::default();
        match Camera::open(0, config) {
            Ok(mut camera) => {
                tracing::info!("camera opened successfully");
                let _ = ready_tx.send(true);
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    match camera.capture_frame() {
                        Ok(frame) => {
                            // Fan out to all registered pipeline inputs
                            {
                                let mut senders = fanout.lock().unwrap();
                                senders.retain(|peer_id, tx| {
                                    if tx.send(frame.clone()).is_err() {
                                        tracing::debug!(
                                            peer_id = %peer_id,
                                            "pipeline frame channel closed, removing"
                                        );
                                        false
                                    } else {
                                        true
                                    }
                                });
                            }
                            // Store for local preview
                            *local_frame.lock().unwrap() = Some(frame);
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "camera frame capture failed");
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(33)); // ~30fps
                }
                tracing::info!("camera capture thread stopped");
            }
            Err(e) => {
                tracing::warn!(error = %e, "failed to open camera");
                let _ = ready_tx.send(false);
            }
        }
    });

    // Wait for camera open result
    ready_rx.recv().unwrap_or(false)
}

/// Stub when the `video` feature is not enabled.
#[cfg(not(feature = "video"))]
fn start_camera_capture(
    _local_frame: &Arc<Mutex<Option<RawFrame>>>,
    _frame_fanout: &FrameFanout,
    _stop_flag: &Arc<std::sync::atomic::AtomicBool>,
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

    #[test]
    fn fanout_delivers_frames_to_multiple_pipelines() {
        let fanout: FrameFanout = Arc::new(Mutex::new(HashMap::new()));

        // Register two pipeline inputs
        let (tx_a, mut rx_a) = mpsc::unbounded_channel();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel();
        {
            let mut senders = fanout.lock().unwrap();
            senders.insert("peer-a".to_string(), tx_a);
            senders.insert("peer-b".to_string(), tx_b);
        }

        // Simulate camera sending a frame
        let frame = RawFrame {
            data: vec![42; 64 * 64 * 4],
            width: 64,
            height: 64,
            format: bisc_media::video_types::PixelFormat::Rgba,
        };
        {
            let senders = fanout.lock().unwrap();
            for (_, tx) in senders.iter() {
                let _ = tx.send(frame.clone());
            }
        }

        // Both pipelines should receive the frame
        let frame_a = rx_a.try_recv().expect("peer-a should receive frame");
        let frame_b = rx_b.try_recv().expect("peer-b should receive frame");
        assert_eq!(frame_a.width, 64);
        assert_eq!(frame_b.width, 64);
        assert_eq!(frame_a.data[0], 42);
        assert_eq!(frame_b.data[0], 42);
    }

    #[test]
    fn fanout_handles_dynamic_add_remove() {
        let fanout: FrameFanout = Arc::new(Mutex::new(HashMap::new()));

        // Start with one pipeline
        let (tx_a, mut rx_a) = mpsc::unbounded_channel();
        fanout.lock().unwrap().insert("peer-a".to_string(), tx_a);

        // Send frame 1 — only peer-a gets it
        let frame1 = RawFrame {
            data: vec![1; 4],
            width: 1,
            height: 1,
            format: bisc_media::video_types::PixelFormat::Rgba,
        };
        {
            let senders = fanout.lock().unwrap();
            for (_, tx) in senders.iter() {
                let _ = tx.send(frame1.clone());
            }
        }
        assert!(rx_a.try_recv().is_ok());

        // Add peer-b
        let (tx_b, mut rx_b) = mpsc::unbounded_channel();
        fanout.lock().unwrap().insert("peer-b".to_string(), tx_b);

        // Send frame 2 — both get it
        let frame2 = RawFrame {
            data: vec![2; 4],
            width: 1,
            height: 1,
            format: bisc_media::video_types::PixelFormat::Rgba,
        };
        {
            let senders = fanout.lock().unwrap();
            for (_, tx) in senders.iter() {
                let _ = tx.send(frame2.clone());
            }
        }
        assert!(rx_a.try_recv().is_ok());
        assert!(rx_b.try_recv().is_ok());

        // Remove peer-a
        fanout.lock().unwrap().remove("peer-a");

        // Send frame 3 — only peer-b gets it
        let frame3 = RawFrame {
            data: vec![3; 4],
            width: 1,
            height: 1,
            format: bisc_media::video_types::PixelFormat::Rgba,
        };
        {
            let senders = fanout.lock().unwrap();
            for (_, tx) in senders.iter() {
                let _ = tx.send(frame3.clone());
            }
        }
        assert!(rx_a.try_recv().is_err()); // peer-a removed
        assert!(rx_b.try_recv().is_ok());
    }

    #[test]
    fn fanout_cleans_up_closed_channels() {
        let fanout: FrameFanout = Arc::new(Mutex::new(HashMap::new()));

        let (tx_a, rx_a) = mpsc::unbounded_channel();
        let (tx_b, mut rx_b) = mpsc::unbounded_channel();
        {
            let mut senders = fanout.lock().unwrap();
            senders.insert("peer-a".to_string(), tx_a);
            senders.insert("peer-b".to_string(), tx_b);
        }

        // Drop peer-a's receiver (simulating pipeline shutdown)
        drop(rx_a);

        // Send a frame — should clean up peer-a's sender
        let frame = RawFrame {
            data: vec![0; 4],
            width: 1,
            height: 1,
            format: bisc_media::video_types::PixelFormat::Rgba,
        };
        {
            let mut senders = fanout.lock().unwrap();
            senders.retain(|_, tx| tx.send(frame.clone()).is_ok());
        }

        // peer-a should be removed, peer-b should still be there
        let senders = fanout.lock().unwrap();
        assert!(!senders.contains_key("peer-a"));
        assert!(senders.contains_key("peer-b"));
        drop(senders);

        // peer-b should have received the frame
        assert!(rx_b.try_recv().is_ok());
    }
}
