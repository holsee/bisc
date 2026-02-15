//! Voice call integration: manages audio I/O, voice pipelines, fan-out, and mixing.
//!
//! When a channel is joined, `VoiceState::init()` opens the default audio devices.
//! As peers connect (`add_peer`), a `VoicePipeline` is created per peer. Microphone
//! audio is fan-out to all pipeline inputs, and all pipeline outputs are mixed before
//! being sent to the speaker.
//!
//! When compiled without the `audio` feature, audio device creation is skipped and
//! voice will be marked unavailable (no audio I/O, but pipelines can still be created
//! for QUIC datagram transport).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bisc_media::voice_pipeline::{VoiceConfig, VoicePipeline};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::settings::Quality;

/// Commands for the audio hub task.
enum HubCommand {
    /// Register a new pipeline's channels.
    AddPipeline {
        peer_id: String,
        mic_tx: mpsc::UnboundedSender<Vec<f32>>,
        speaker_rx: mpsc::UnboundedReceiver<Vec<f32>>,
    },
    /// Remove a pipeline's channels.
    RemovePipeline { peer_id: String },
    /// Shut down the hub.
    Shutdown,
}

/// Map a `Quality` preset to a `VoiceConfig` with appropriate bitrate bounds.
pub fn voice_config_for_quality(quality: Quality) -> VoiceConfig {
    match quality {
        Quality::Low => VoiceConfig {
            initial_bitrate_bps: 24_000,
            max_bitrate_bps: 32_000,
            min_bitrate_bps: 12_000,
        },
        Quality::Medium => VoiceConfig {
            initial_bitrate_bps: 64_000,
            max_bitrate_bps: 96_000,
            min_bitrate_bps: 24_000,
        },
        Quality::High => VoiceConfig {
            initial_bitrate_bps: 128_000,
            max_bitrate_bps: 128_000,
            min_bitrate_bps: 48_000,
        },
    }
}

/// Manages voice state: audio devices, pipelines, fan-out, and mixing.
pub struct VoiceState {
    /// Active voice pipelines keyed by peer hex ID.
    pipelines: HashMap<String, VoicePipeline>,
    /// Command sender for the audio hub task.
    hub_cmd_tx: Option<mpsc::UnboundedSender<HubCommand>>,
    /// Audio hub task handle.
    hub_handle: Option<JoinHandle<()>>,
    /// Whether audio devices were successfully opened.
    audio_available: bool,
    /// Global mute state.
    muted: bool,
    /// Current audio quality preset (used when creating new pipelines).
    quality: Quality,
}

impl VoiceState {
    /// Create a new voice state (not yet initialized).
    pub fn new() -> Self {
        Self {
            pipelines: HashMap::new(),
            hub_cmd_tx: None,
            hub_handle: None,
            audio_available: false,
            muted: false,
            quality: Quality::Medium,
        }
    }

    /// Set the audio quality preset. Affects newly created pipelines.
    pub fn set_quality(&mut self, quality: Quality) {
        tracing::info!(?quality, "voice quality preset changed");
        self.quality = quality;
    }

    /// Initialize audio devices and start the audio hub.
    ///
    /// Gracefully handles missing audio hardware by setting `audio_available = false`.
    pub fn init(&mut self) {
        if self.hub_cmd_tx.is_some() {
            tracing::debug!("voice state already initialized");
            return;
        }

        let (mic_rx, speaker_tx) = init_audio_devices();
        self.audio_available = mic_rx.is_some() || speaker_tx.is_some();

        if !self.audio_available {
            tracing::warn!("no audio devices available, voice will be disabled");
            return;
        }

        let (hub_cmd_tx, hub_cmd_rx) = mpsc::unbounded_channel();
        self.hub_cmd_tx = Some(hub_cmd_tx);

        self.hub_handle = Some(tokio::spawn(audio_hub_task(mic_rx, speaker_tx, hub_cmd_rx)));

        tracing::info!(
            audio_available = self.audio_available,
            "voice state initialized"
        );
    }

    /// Add a voice pipeline for a newly connected peer.
    pub async fn add_peer(
        &mut self,
        peer_id: String,
        connection: iroh::endpoint::Connection,
    ) -> anyhow::Result<()> {
        if self.pipelines.contains_key(&peer_id) {
            tracing::debug!(peer_id = %peer_id, "pipeline already exists for peer");
            return Ok(());
        }

        if !self.audio_available {
            tracing::warn!(
                peer_id = %peer_id,
                "skipping voice pipeline: no audio devices"
            );
            return Ok(());
        }

        // Create channels for this pipeline
        let (mic_tx, mic_rx) = mpsc::unbounded_channel();
        let (speaker_tx, speaker_rx) = mpsc::unbounded_channel();

        // Create and start the pipeline with quality-derived config
        let config = voice_config_for_quality(self.quality);
        tracing::debug!(
            peer_id = %peer_id,
            initial_bitrate = config.initial_bitrate_bps,
            max_bitrate = config.max_bitrate_bps,
            min_bitrate = config.min_bitrate_bps,
            "creating voice pipeline with config"
        );
        let mut pipeline = VoicePipeline::new(connection, config, mic_rx, speaker_tx)?;
        pipeline.set_muted(self.muted);
        pipeline.start().await?;

        tracing::info!(peer_id = %peer_id, "voice pipeline started for peer");

        // Register with the audio hub
        if let Some(hub_tx) = &self.hub_cmd_tx {
            let _ = hub_tx.send(HubCommand::AddPipeline {
                peer_id: peer_id.clone(),
                mic_tx,
                speaker_rx,
            });
        }

        self.pipelines.insert(peer_id, pipeline);
        Ok(())
    }

    /// Remove and stop the voice pipeline for a peer.
    pub async fn remove_peer(&mut self, peer_id: &str) {
        if let Some(mut pipeline) = self.pipelines.remove(peer_id) {
            pipeline.stop().await;
            tracing::info!(peer_id = %peer_id, "voice pipeline stopped for peer");
        }

        // Notify the hub to remove channels
        if let Some(hub_tx) = &self.hub_cmd_tx {
            let _ = hub_tx.send(HubCommand::RemovePipeline {
                peer_id: peer_id.to_string(),
            });
        }
    }

    /// Set mute state on all active pipelines.
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        for pipeline in self.pipelines.values() {
            pipeline.set_muted(muted);
        }
        tracing::info!(
            muted,
            pipelines = self.pipelines.len(),
            "voice mute state changed"
        );
    }

    /// Check if voice is currently muted.
    #[allow(dead_code)]
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Get pipeline metrics for a specific peer.
    #[allow(dead_code)]
    pub fn pipeline_metrics(
        &self,
        peer_id: &str,
    ) -> Option<&Arc<bisc_media::voice_pipeline::PipelineMetrics>> {
        self.pipelines.get(peer_id).map(|p| p.metrics())
    }

    /// Shut down all voice pipelines and the audio hub.
    pub async fn shutdown(&mut self) {
        // Stop all pipelines
        for (peer_id, mut pipeline) in self.pipelines.drain() {
            pipeline.stop().await;
            tracing::debug!(peer_id = %peer_id, "voice pipeline stopped on shutdown");
        }

        // Shut down the hub
        if let Some(hub_tx) = self.hub_cmd_tx.take() {
            let _ = hub_tx.send(HubCommand::Shutdown);
        }
        if let Some(handle) = self.hub_handle.take() {
            let _ = handle.await;
        }

        self.audio_available = false;
        tracing::info!("voice state shut down");
    }
}

/// Initialize audio input and output devices.
///
/// Returns (mic_receiver, speaker_sender). Each is `None` if the device
/// could not be opened (no hardware, or `audio` feature disabled).
#[cfg(feature = "audio")]
#[allow(clippy::type_complexity)]
fn init_audio_devices() -> (
    Option<mpsc::UnboundedReceiver<Vec<f32>>>,
    Option<mpsc::UnboundedSender<Vec<f32>>>,
) {
    use bisc_media::audio_io::{AudioConfig, AudioInput, AudioOutput};

    let audio_config = AudioConfig::default();

    let mic_rx = match AudioInput::new(&audio_config) {
        Ok((input, rx)) => {
            if let Err(e) = input.start() {
                tracing::warn!(error = %e, "failed to start audio input");
                None
            } else {
                // Leak the AudioInput to keep the cpal stream alive.
                // It will be cleaned up when the process exits or the
                // channel receiver is dropped (causing the stream callback
                // to stop sending).
                std::mem::forget(input);
                Some(rx)
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "no audio input device available");
            None
        }
    };

    let speaker_tx = match AudioOutput::new(&audio_config) {
        Ok(output) => {
            if let Err(e) = output.start() {
                tracing::warn!(error = %e, "failed to start audio output");
                None
            } else {
                let tx = output.sender();
                // Leak the AudioOutput to keep the cpal stream alive.
                std::mem::forget(output);
                Some(tx)
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "no audio output device available");
            None
        }
    };

    (mic_rx, speaker_tx)
}

/// Stub when the `audio` feature is not enabled.
#[cfg(not(feature = "audio"))]
#[allow(clippy::type_complexity)]
fn init_audio_devices() -> (
    Option<mpsc::UnboundedReceiver<Vec<f32>>>,
    Option<mpsc::UnboundedSender<Vec<f32>>>,
) {
    tracing::info!("audio feature not enabled, voice disabled");
    (None, None)
}

/// Audio hub task: fans out microphone audio to all pipelines and mixes
/// pipeline outputs to the speaker.
async fn audio_hub_task(
    mut mic_rx: Option<mpsc::UnboundedReceiver<Vec<f32>>>,
    speaker_tx: Option<mpsc::UnboundedSender<Vec<f32>>>,
    mut cmd_rx: mpsc::UnboundedReceiver<HubCommand>,
) {
    // Pipeline channels
    let mut pipeline_inputs: HashMap<String, mpsc::UnboundedSender<Vec<f32>>> = HashMap::new();
    let mut pipeline_outputs: HashMap<String, mpsc::UnboundedReceiver<Vec<f32>>> = HashMap::new();

    let mut mix_interval = tokio::time::interval(Duration::from_millis(20));
    mix_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    tracing::debug!("audio hub task started");

    loop {
        tokio::select! {
            biased;

            // Handle commands (add/remove pipelines, shutdown)
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(HubCommand::AddPipeline { peer_id, mic_tx, speaker_rx }) => {
                        tracing::debug!(peer_id = %peer_id, "hub: adding pipeline");
                        pipeline_inputs.insert(peer_id.clone(), mic_tx);
                        pipeline_outputs.insert(peer_id, speaker_rx);
                    }
                    Some(HubCommand::RemovePipeline { peer_id }) => {
                        tracing::debug!(peer_id = %peer_id, "hub: removing pipeline");
                        pipeline_inputs.remove(&peer_id);
                        pipeline_outputs.remove(&peer_id);
                    }
                    Some(HubCommand::Shutdown) | None => {
                        tracing::debug!("audio hub shutting down");
                        break;
                    }
                }
            }

            // Fan-out: mic frames → all pipeline inputs
            Some(frame) = async {
                match &mut mic_rx {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                // Send to each pipeline, removing dead ones
                pipeline_inputs.retain(|_, tx| tx.send(frame.clone()).is_ok());
            }

            // Mix: collect from all pipeline outputs, mix, send to speaker
            _ = mix_interval.tick() => {
                if pipeline_outputs.is_empty() || speaker_tx.is_none() {
                    continue;
                }

                let mut mixed: Option<Vec<f32>> = None;
                let mut dead_peers = Vec::new();

                for (peer_id, rx) in pipeline_outputs.iter_mut() {
                    match rx.try_recv() {
                        Ok(samples) => {
                            match &mut mixed {
                                None => mixed = Some(samples),
                                Some(mix) => {
                                    // Additive mixing with clipping prevention
                                    for (i, s) in samples.iter().enumerate() {
                                        if i < mix.len() {
                                            mix[i] = (mix[i] + s).clamp(-1.0, 1.0);
                                        }
                                    }
                                }
                            }
                        }
                        Err(mpsc::error::TryRecvError::Empty) => {}
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            dead_peers.push(peer_id.clone());
                        }
                    }
                }

                // Clean up dead pipeline outputs
                for peer_id in dead_peers {
                    pipeline_outputs.remove(&peer_id);
                    pipeline_inputs.remove(&peer_id);
                }

                // Send mixed audio to speaker
                if let (Some(samples), Some(tx)) = (mixed, &speaker_tx) {
                    let _ = tx.send(samples);
                }
            }
        }
    }

    tracing::debug!("audio hub task ended");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_state_starts_uninitialised() {
        let state = VoiceState::new();
        assert!(!state.audio_available);
        assert!(!state.muted);
        assert!(state.pipelines.is_empty());
        assert!(state.hub_cmd_tx.is_none());
    }

    #[test]
    fn mute_state_toggles() {
        let mut state = VoiceState::new();
        assert!(!state.is_muted());
        state.set_muted(true);
        assert!(state.is_muted());
        state.set_muted(false);
        assert!(!state.is_muted());
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let mut state = VoiceState::new();
        // Shutdown before init should not panic
        state.shutdown().await;
        assert!(!state.audio_available);
    }

    #[tokio::test]
    async fn remove_nonexistent_peer_is_noop() {
        let mut state = VoiceState::new();
        // Should not panic
        state.remove_peer("nonexistent").await;
    }

    #[tokio::test]
    async fn hub_task_processes_commands() {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        // Start hub with no audio devices
        let handle = tokio::spawn(audio_hub_task(None, None, cmd_rx));

        // Add and remove a pipeline (with dummy channels)
        let (mic_tx, _mic_rx) = mpsc::unbounded_channel();
        let (_speaker_tx, speaker_rx) = mpsc::unbounded_channel();
        cmd_tx
            .send(HubCommand::AddPipeline {
                peer_id: "test-peer".to_string(),
                mic_tx,
                speaker_rx,
            })
            .unwrap();

        cmd_tx
            .send(HubCommand::RemovePipeline {
                peer_id: "test-peer".to_string(),
            })
            .unwrap();

        cmd_tx.send(HubCommand::Shutdown).unwrap();

        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("hub should shut down")
            .expect("hub should not panic");
    }

    #[tokio::test]
    async fn hub_fan_out_distributes_mic_frames() {
        let (mic_in_tx, mic_in_rx) = mpsc::unbounded_channel::<Vec<f32>>();
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();

        // Start hub with a mic source but no speaker
        let handle = tokio::spawn(audio_hub_task(Some(mic_in_rx), None, cmd_rx));

        // Create two pipeline inputs
        let (pipe_a_tx, mut pipe_a_rx) = mpsc::unbounded_channel();
        let (_pipe_a_spk_tx, pipe_a_spk_rx) = mpsc::unbounded_channel();
        cmd_tx
            .send(HubCommand::AddPipeline {
                peer_id: "peer-a".to_string(),
                mic_tx: pipe_a_tx,
                speaker_rx: pipe_a_spk_rx,
            })
            .unwrap();

        let (pipe_b_tx, mut pipe_b_rx) = mpsc::unbounded_channel();
        let (_pipe_b_spk_tx, pipe_b_spk_rx) = mpsc::unbounded_channel();
        cmd_tx
            .send(HubCommand::AddPipeline {
                peer_id: "peer-b".to_string(),
                mic_tx: pipe_b_tx,
                speaker_rx: pipe_b_spk_rx,
            })
            .unwrap();

        // Give the hub time to process commands
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Send a mic frame
        let frame = vec![0.5_f32; 960];
        mic_in_tx.send(frame.clone()).unwrap();

        // Give hub time to fan out
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Both pipelines should receive the frame
        let a_frame = pipe_a_rx.try_recv().expect("peer-a should receive frame");
        let b_frame = pipe_b_rx.try_recv().expect("peer-b should receive frame");
        assert_eq!(a_frame, frame);
        assert_eq!(b_frame, frame);

        cmd_tx.send(HubCommand::Shutdown).unwrap();

        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("hub should shut down")
            .expect("hub should not panic");
    }

    #[tokio::test]
    async fn hub_mixing_combines_outputs() {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (speaker_tx, mut speaker_rx) = mpsc::unbounded_channel::<Vec<f32>>();

        // Start hub with a speaker but no mic
        let handle = tokio::spawn(audio_hub_task(None, Some(speaker_tx), cmd_rx));

        // Create two pipelines with speaker outputs
        let (_pipe_a_mic_tx, _pipe_a_mic_rx) = mpsc::unbounded_channel();
        let (pipe_a_spk_tx, pipe_a_spk_rx) = mpsc::unbounded_channel();
        cmd_tx
            .send(HubCommand::AddPipeline {
                peer_id: "peer-a".to_string(),
                mic_tx: _pipe_a_mic_tx,
                speaker_rx: pipe_a_spk_rx,
            })
            .unwrap();

        let (_pipe_b_mic_tx, _pipe_b_mic_rx) = mpsc::unbounded_channel();
        let (pipe_b_spk_tx, pipe_b_spk_rx) = mpsc::unbounded_channel();
        cmd_tx
            .send(HubCommand::AddPipeline {
                peer_id: "peer-b".to_string(),
                mic_tx: _pipe_b_mic_tx,
                speaker_rx: pipe_b_spk_rx,
            })
            .unwrap();

        // Give hub time to process commands
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Send audio from both pipelines
        pipe_a_spk_tx.send(vec![0.5; 960]).unwrap();
        pipe_b_spk_tx.send(vec![0.3; 960]).unwrap();

        // Wait for mix interval to fire (20ms) plus some margin
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Speaker should receive mixed audio
        let mixed = speaker_rx.try_recv().expect("should receive mixed audio");
        assert_eq!(mixed.len(), 960);
        // 0.5 + 0.3 = 0.8
        assert!((mixed[0] - 0.8).abs() < 0.01);

        cmd_tx.send(HubCommand::Shutdown).unwrap();

        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("hub should shut down")
            .expect("hub should not panic");
    }

    #[tokio::test]
    async fn hub_mixing_clamps_to_prevent_clipping() {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (speaker_tx, mut speaker_rx) = mpsc::unbounded_channel::<Vec<f32>>();

        let handle = tokio::spawn(audio_hub_task(None, Some(speaker_tx), cmd_rx));

        let (_pipe_a_mic_tx, _) = mpsc::unbounded_channel();
        let (pipe_a_spk_tx, pipe_a_spk_rx) = mpsc::unbounded_channel();
        cmd_tx
            .send(HubCommand::AddPipeline {
                peer_id: "peer-a".to_string(),
                mic_tx: _pipe_a_mic_tx,
                speaker_rx: pipe_a_spk_rx,
            })
            .unwrap();

        let (_pipe_b_mic_tx, _) = mpsc::unbounded_channel();
        let (pipe_b_spk_tx, pipe_b_spk_rx) = mpsc::unbounded_channel();
        cmd_tx
            .send(HubCommand::AddPipeline {
                peer_id: "peer-b".to_string(),
                mic_tx: _pipe_b_mic_tx,
                speaker_rx: pipe_b_spk_rx,
            })
            .unwrap();

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Both at 0.8 → sum 1.6 → should be clamped to 1.0
        pipe_a_spk_tx.send(vec![0.8; 960]).unwrap();
        pipe_b_spk_tx.send(vec![0.8; 960]).unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;

        let mixed = speaker_rx.try_recv().expect("should receive mixed audio");
        assert_eq!(mixed.len(), 960);
        // 0.8 + 0.8 = 1.6 → clamped to 1.0
        assert!((mixed[0] - 1.0).abs() < 0.01);

        cmd_tx.send(HubCommand::Shutdown).unwrap();
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("hub should shut down")
            .expect("hub should not panic");
    }
}
