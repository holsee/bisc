//! End-to-end voice call pipeline: capture → encode → send → receive → decode → playback.
//!
//! `VoicePipeline` wires together all media components (`OpusEncoder`, `OpusDecoder`,
//! `MediaTransport`, `JitterBuffer`) into two async background tasks (send + receive)
//! communicating through channels. Audio I/O is abstracted via `mpsc` channels so the
//! pipeline works both with real hardware and with test injectors.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use bisc_protocol::media::MediaPacket;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::jitter_buffer::JitterBuffer;
use crate::opus_codec::{OpusDecoder, OpusEncoder, SAMPLES_PER_FRAME};
use crate::transport::MediaTransport;

/// Commands that can be sent to the voice send loop.
#[derive(Debug)]
pub enum VoiceCommand {
    /// Change the encoder's target bitrate (bps).
    SetBitrate(u32),
}

/// Metrics exposed for observability and test assertions.
pub struct PipelineMetrics {
    pub frames_encoded: AtomicU64,
    pub frames_decoded: AtomicU64,
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
}

impl PipelineMetrics {
    fn new() -> Self {
        Self {
            frames_encoded: AtomicU64::new(0),
            frames_decoded: AtomicU64::new(0),
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
        }
    }
}

/// End-to-end voice pipeline orchestrating send and receive loops.
///
/// The send loop reads audio frames from `audio_in_rx`, encodes them with Opus,
/// wraps them in `MediaPacket`s, and sends them via `MediaTransport`.
///
/// The receive loop reads packets from the transport, pushes them through a
/// `JitterBuffer`, decodes with Opus, applies volume gain, and sends decoded
/// samples to `audio_out_tx`.
pub struct VoicePipeline {
    connection: iroh::endpoint::Connection,
    audio_in_rx: Option<mpsc::UnboundedReceiver<Vec<f32>>>,
    audio_out_tx: Option<mpsc::UnboundedSender<Vec<f32>>>,
    muted: Arc<AtomicBool>,
    /// Volume gain stored as `f32::to_bits()` in an `AtomicU32`.
    volume_bits: Arc<AtomicU32>,
    command_tx: mpsc::UnboundedSender<VoiceCommand>,
    metrics: Arc<PipelineMetrics>,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    send_handle: Option<JoinHandle<()>>,
    recv_handle: Option<JoinHandle<()>>,
}

impl VoicePipeline {
    /// Create a pipeline from a QUIC connection and audio channels.
    ///
    /// - `audio_in_rx`: provides captured audio frames (from `AudioInput` or test injector).
    /// - `audio_out_tx`: receives decoded frames (to `AudioOutput` or test collector).
    pub fn new(
        connection: iroh::endpoint::Connection,
        audio_in_rx: mpsc::UnboundedReceiver<Vec<f32>>,
        audio_out_tx: mpsc::UnboundedSender<Vec<f32>>,
    ) -> Result<Self> {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (command_tx, _command_rx) = mpsc::unbounded_channel();

        tracing::info!(
            peer = %connection.remote_id(),
            "voice pipeline created"
        );

        Ok(Self {
            connection,
            audio_in_rx: Some(audio_in_rx),
            audio_out_tx: Some(audio_out_tx),
            muted: Arc::new(AtomicBool::new(false)),
            volume_bits: Arc::new(AtomicU32::new(1.0_f32.to_bits())),
            command_tx,
            metrics: Arc::new(PipelineMetrics::new()),
            shutdown_tx,
            shutdown_rx,
            send_handle: None,
            recv_handle: None,
        })
    }

    /// Spawn the send and receive background tasks.
    pub async fn start(&mut self) -> Result<()> {
        let audio_in_rx = self
            .audio_in_rx
            .take()
            .context("pipeline already started (audio_in_rx consumed)")?;
        let audio_out_tx = self
            .audio_out_tx
            .take()
            .context("pipeline already started (audio_out_tx consumed)")?;

        // Create a fresh command channel for the send loop
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        self.command_tx = command_tx;

        // --- Send task ---
        let send_handle = tokio::spawn(send_loop(
            self.connection.clone(),
            audio_in_rx,
            command_rx,
            Arc::clone(&self.muted),
            Arc::clone(&self.metrics),
            self.shutdown_rx.clone(),
        ));

        // --- Receive task ---
        let recv_handle = tokio::spawn(recv_loop(
            self.connection.clone(),
            audio_out_tx,
            Arc::clone(&self.volume_bits),
            Arc::clone(&self.metrics),
            self.shutdown_rx.clone(),
        ));

        self.send_handle = Some(send_handle);
        self.recv_handle = Some(recv_handle);

        tracing::info!("voice pipeline started");
        Ok(())
    }

    /// Signal shutdown and await completion of background tasks.
    pub async fn stop(&mut self) {
        tracing::info!("stopping voice pipeline");
        let _ = self.shutdown_tx.send(true);

        if let Some(h) = self.send_handle.take() {
            let _ = h.await;
        }
        if let Some(h) = self.recv_handle.take() {
            let _ = h.await;
        }
        tracing::info!("voice pipeline stopped");
    }

    /// Toggle mute. When muted, the send loop drops frames instead of encoding.
    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
        tracing::debug!(muted, "mute state changed");
    }

    /// Check whether the pipeline is muted.
    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::Relaxed)
    }

    /// Set the playback volume gain (1.0 = unity).
    pub fn set_volume(&self, gain: f32) {
        self.volume_bits.store(gain.to_bits(), Ordering::Relaxed);
        tracing::debug!(gain, "volume changed");
    }

    /// Get the current playback volume gain.
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume_bits.load(Ordering::Relaxed))
    }

    /// Change the encoder's target bitrate.
    pub fn set_bitrate(&self, bps: u32) {
        let _ = self.command_tx.send(VoiceCommand::SetBitrate(bps));
    }

    /// Get the pipeline metrics.
    pub fn metrics(&self) -> &Arc<PipelineMetrics> {
        &self.metrics
    }
}

/// Send loop: read audio frames → encode → packetize → send.
async fn send_loop(
    connection: iroh::endpoint::Connection,
    mut audio_in_rx: mpsc::UnboundedReceiver<Vec<f32>>,
    mut command_rx: mpsc::UnboundedReceiver<VoiceCommand>,
    muted: Arc<AtomicBool>,
    metrics: Arc<PipelineMetrics>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut encoder = match OpusEncoder::new(1, 64_000) {
        Ok(e) => e,
        Err(e) => {
            tracing::error!(error = %e, "failed to create Opus encoder");
            return;
        }
    };
    let mut transport = MediaTransport::new(connection);
    let mut timestamp: u32 = 0;

    tracing::debug!("send loop started");

    loop {
        tokio::select! {
            biased;
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    tracing::debug!("send loop shutdown");
                    return;
                }
            }
            cmd = command_rx.recv() => {
                match cmd {
                    Some(VoiceCommand::SetBitrate(bps)) => {
                        if let Err(e) = encoder.set_bitrate(bps) {
                            tracing::warn!(error = %e, bps, "failed to set voice bitrate");
                        }
                    }
                    None => {
                        tracing::debug!("voice command channel closed");
                        return;
                    }
                }
            }
            frame = audio_in_rx.recv() => {
                let Some(samples) = frame else {
                    tracing::debug!("audio input channel closed");
                    return;
                };

                if muted.load(Ordering::Relaxed) {
                    timestamp = timestamp.wrapping_add(SAMPLES_PER_FRAME as u32);
                    continue;
                }

                match encoder.encode(&samples) {
                    Ok(encoded) => {
                        metrics.frames_encoded.fetch_add(1, Ordering::Relaxed);

                        let mut packet = MediaPacket {
                            stream_id: 0,
                            sequence: 0, // assigned by transport
                            timestamp,
                            fragment_index: 0,
                            fragment_count: 1,
                            is_keyframe: false,
                            payload: encoded,
                        };

                        if let Err(e) = transport.send_media_packet(&mut packet) {
                            tracing::warn!(error = %e, "failed to send media packet");
                            return;
                        }
                        metrics.packets_sent.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "opus encode failed, skipping frame");
                    }
                }

                timestamp = timestamp.wrapping_add(SAMPLES_PER_FRAME as u32);
            }
        }
    }
}

/// Receive loop: recv packets → jitter buffer → decode → apply volume → output.
async fn recv_loop(
    connection: iroh::endpoint::Connection,
    audio_out_tx: mpsc::UnboundedSender<Vec<f32>>,
    volume_bits: Arc<AtomicU32>,
    metrics: Arc<PipelineMetrics>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut decoder = match OpusDecoder::new(1) {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "failed to create Opus decoder");
            return;
        }
    };
    let mut transport = MediaTransport::new(connection);
    let mut jitter_buffer = JitterBuffer::new(0, SAMPLES_PER_FRAME as u32, 20);

    let mut playout_interval = tokio::time::interval(Duration::from_millis(20));
    playout_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    tracing::debug!("receive loop started");

    loop {
        tokio::select! {
            biased;
            result = shutdown.changed() => {
                if result.is_err() || *shutdown.borrow() {
                    tracing::debug!("receive loop shutdown");
                    return;
                }
            }
            result = transport.recv_media_packet() => {
                match result {
                    Ok(packet) => {
                        metrics.packets_received.fetch_add(1, Ordering::Relaxed);
                        jitter_buffer.push(packet);
                    }
                    Err(e) => {
                        tracing::debug!(error = %e, "transport recv ended");
                        return;
                    }
                }
            }
            _ = playout_interval.tick() => {
                if let Some(packet) = jitter_buffer.pop() {
                    match decoder.decode(&packet.payload) {
                        Ok(mut samples) => {
                            let gain = f32::from_bits(volume_bits.load(Ordering::Relaxed));
                            if (gain - 1.0).abs() > f32::EPSILON {
                                apply_volume(&mut samples, gain);
                            }
                            metrics.frames_decoded.fetch_add(1, Ordering::Relaxed);
                            if audio_out_tx.send(samples).is_err() {
                                tracing::debug!("audio output channel closed");
                                return;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "opus decode failed, skipping frame");
                        }
                    }
                }
            }
        }
    }
}

/// Apply volume gain to a buffer of audio samples.
fn apply_volume(samples: &mut [f32], gain: f32) {
    for s in samples.iter_mut() {
        *s *= gain;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_gain_applied_correctly() {
        let mut samples = vec![0.5, -0.5, 1.0, -1.0, 0.0];
        apply_volume(&mut samples, 2.0);
        assert_eq!(samples, vec![1.0, -1.0, 2.0, -2.0, 0.0]);
    }

    #[test]
    fn volume_gain_unity_is_noop() {
        let original = vec![0.5, -0.5, 1.0];
        let mut samples = original.clone();
        apply_volume(&mut samples, 1.0);
        assert_eq!(samples, original);
    }

    #[test]
    fn volume_gain_zero_silences() {
        let mut samples = vec![0.5, -0.5, 1.0, -1.0];
        apply_volume(&mut samples, 0.0);
        assert_eq!(samples, vec![0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn mute_flag_toggles() {
        let muted = AtomicBool::new(false);
        assert!(!muted.load(Ordering::Relaxed));
        muted.store(true, Ordering::Relaxed);
        assert!(muted.load(Ordering::Relaxed));
        muted.store(false, Ordering::Relaxed);
        assert!(!muted.load(Ordering::Relaxed));
    }

    #[test]
    fn volume_stored_as_atomic_bits() {
        let vol = AtomicU32::new(1.0_f32.to_bits());
        let val = f32::from_bits(vol.load(Ordering::Relaxed));
        assert!((val - 1.0).abs() < f32::EPSILON);

        vol.store(0.5_f32.to_bits(), Ordering::Relaxed);
        let val = f32::from_bits(vol.load(Ordering::Relaxed));
        assert!((val - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn pipeline_metrics_increment() {
        let m = PipelineMetrics::new();
        m.frames_encoded.fetch_add(1, Ordering::Relaxed);
        m.frames_decoded.fetch_add(2, Ordering::Relaxed);
        m.packets_sent.fetch_add(3, Ordering::Relaxed);
        m.packets_received.fetch_add(4, Ordering::Relaxed);
        assert_eq!(m.frames_encoded.load(Ordering::Relaxed), 1);
        assert_eq!(m.frames_decoded.load(Ordering::Relaxed), 2);
        assert_eq!(m.packets_sent.load(Ordering::Relaxed), 3);
        assert_eq!(m.packets_received.load(Ordering::Relaxed), 4);
    }
}
