//! Audio I/O using `cpal` for microphone capture and speaker playback.
//!
//! Requires the `audio` feature to be enabled.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use tokio::sync::mpsc;

/// Standard sample rate for bisc audio (matches Opus default).
pub const SAMPLE_RATE: u32 = 48_000;

/// Standard frame duration in milliseconds (20ms = 960 samples at 48kHz).
pub const FRAME_DURATION_MS: u32 = 20;

/// Number of samples per frame at the standard sample rate.
pub const SAMPLES_PER_FRAME: usize = (SAMPLE_RATE * FRAME_DURATION_MS / 1000) as usize;

/// Audio configuration.
#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Number of channels (1 = mono, 2 = stereo).
    pub channels: u16,
    /// Buffer size in frames (0 = default).
    pub buffer_size: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: SAMPLE_RATE,
            channels: 1,
            buffer_size: 0,
        }
    }
}

/// Information about an available audio device.
#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    /// Human-readable device name.
    pub name: String,
}

/// List available audio input (microphone) devices.
pub fn list_input_devices() -> Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .context("failed to enumerate input devices")?;

    let mut result = Vec::new();
    for device in devices {
        let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        result.push(AudioDeviceInfo { name });
    }

    tracing::debug!(count = result.len(), "enumerated input devices");
    Ok(result)
}

/// List available audio output (speaker) devices.
pub fn list_output_devices() -> Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();
    let devices = host
        .output_devices()
        .context("failed to enumerate output devices")?;

    let mut result = Vec::new();
    for device in devices {
        let name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        result.push(AudioDeviceInfo { name });
    }

    tracing::debug!(count = result.len(), "enumerated output devices");
    Ok(result)
}

/// Audio input (microphone capture).
///
/// Captures audio samples from the default input device and sends them
/// as `Vec<f32>` buffers through a channel.
pub struct AudioInput {
    stream: cpal::Stream,
    running: Arc<AtomicBool>,
}

impl AudioInput {
    /// Create a new audio input using the default device.
    ///
    /// Returns the input and a receiver for sample buffers.
    /// Each buffer contains `SAMPLES_PER_FRAME * channels` samples.
    pub fn new(config: &AudioConfig) -> Result<(Self, mpsc::UnboundedReceiver<Vec<f32>>)> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .context("no default input device available")?;

        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        tracing::info!(device = %device_name, "opening audio input");

        let stream_config = cpal::StreamConfig {
            channels: config.channels,
            sample_rate: cpal::SampleRate(config.sample_rate),
            buffer_size: if config.buffer_size > 0 {
                cpal::BufferSize::Fixed(config.buffer_size)
            } else {
                cpal::BufferSize::Default
            },
        };

        let (tx, rx) = mpsc::unbounded_channel();
        let running = Arc::new(AtomicBool::new(false));
        let running_clone = running.clone();

        let samples_per_frame =
            (config.sample_rate * FRAME_DURATION_MS / 1000) as usize * config.channels as usize;
        let mut accumulator: Vec<f32> = Vec::with_capacity(samples_per_frame);

        let stream = device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                if !running_clone.load(Ordering::Relaxed) {
                    return;
                }

                // Accumulate samples into frame-sized buffers
                let mut offset = 0;
                while offset < data.len() {
                    let remaining = samples_per_frame - accumulator.len();
                    let to_copy = remaining.min(data.len() - offset);
                    accumulator.extend_from_slice(&data[offset..offset + to_copy]);
                    offset += to_copy;

                    if accumulator.len() >= samples_per_frame {
                        let frame = std::mem::replace(
                            &mut accumulator,
                            Vec::with_capacity(samples_per_frame),
                        );
                        if tx.send(frame).is_err() {
                            tracing::debug!("audio input receiver dropped");
                            return;
                        }
                    }
                }
            },
            move |err| {
                tracing::error!(error = %err, "audio input stream error");
            },
            None,
        )?;

        Ok((Self { stream, running }, rx))
    }

    /// Start capturing audio.
    pub fn start(&self) -> Result<()> {
        self.running.store(true, Ordering::Relaxed);
        self.stream.play().context("failed to start audio input")?;
        tracing::info!("audio input started");
        Ok(())
    }

    /// Stop capturing audio.
    pub fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);
        self.stream.pause().context("failed to pause audio input")?;
        tracing::info!("audio input stopped");
        Ok(())
    }

    /// Check if the input is currently capturing.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

/// Audio output (speaker playback).
///
/// Plays audio samples on the default output device, consuming
/// `Vec<f32>` buffers from a channel.
pub struct AudioOutput {
    stream: cpal::Stream,
    running: Arc<AtomicBool>,
    tx: mpsc::UnboundedSender<Vec<f32>>,
}

impl AudioOutput {
    /// Create a new audio output using the default device.
    pub fn new(config: &AudioConfig) -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no default output device available")?;

        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        tracing::info!(device = %device_name, "opening audio output");

        let stream_config = cpal::StreamConfig {
            channels: config.channels,
            sample_rate: cpal::SampleRate(config.sample_rate),
            buffer_size: if config.buffer_size > 0 {
                cpal::BufferSize::Fixed(config.buffer_size)
            } else {
                cpal::BufferSize::Default
            },
        };

        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<f32>>();
        let running = Arc::new(AtomicBool::new(false));
        let running_clone = running.clone();

        // Buffer for samples waiting to be played
        let mut playback_buffer: Vec<f32> = Vec::new();

        let stream = device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                if !running_clone.load(Ordering::Relaxed) {
                    data.fill(0.0);
                    return;
                }

                // Fill output buffer from received frames
                let mut written = 0;
                while written < data.len() {
                    if playback_buffer.is_empty() {
                        match rx.try_recv() {
                            Ok(frame) => {
                                playback_buffer = frame;
                            }
                            Err(_) => {
                                // No data available — output silence
                                data[written..].fill(0.0);
                                return;
                            }
                        }
                    }

                    let available = playback_buffer.len();
                    let needed = data.len() - written;
                    let to_copy = available.min(needed);

                    data[written..written + to_copy].copy_from_slice(&playback_buffer[..to_copy]);
                    playback_buffer.drain(..to_copy);
                    written += to_copy;
                }
            },
            move |err| {
                tracing::error!(error = %err, "audio output stream error");
            },
            None,
        )?;

        Ok(Self {
            stream,
            running,
            tx,
        })
    }

    /// Start playing audio.
    pub fn start(&self) -> Result<()> {
        self.running.store(true, Ordering::Relaxed);
        self.stream.play().context("failed to start audio output")?;
        tracing::info!("audio output started");
        Ok(())
    }

    /// Stop playing audio.
    pub fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);
        self.stream
            .pause()
            .context("failed to pause audio output")?;
        tracing::info!("audio output stopped");
        Ok(())
    }

    /// Check if the output is currently playing.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get a sender to submit audio frames for playback.
    pub fn sender(&self) -> mpsc::UnboundedSender<Vec<f32>> {
        self.tx.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_input_devices_does_not_panic() {
        // May return empty list if no audio hardware is present
        match list_input_devices() {
            Ok(devices) => {
                println!("Found {} input devices", devices.len());
                for d in &devices {
                    println!("  - {}", d.name);
                }
            }
            Err(e) => {
                // On systems without audio support, this is acceptable
                println!("Could not enumerate input devices: {e}");
            }
        }
    }

    #[test]
    fn list_output_devices_does_not_panic() {
        match list_output_devices() {
            Ok(devices) => {
                println!("Found {} output devices", devices.len());
                for d in &devices {
                    println!("  - {}", d.name);
                }
            }
            Err(e) => {
                println!("Could not enumerate output devices: {e}");
            }
        }
    }

    #[test]
    fn audio_config_default_is_48k_mono() {
        let config = AudioConfig::default();
        assert_eq!(config.sample_rate, 48_000);
        assert_eq!(config.channels, 1);
        assert_eq!(config.buffer_size, 0);
    }

    #[test]
    fn samples_per_frame_is_960() {
        assert_eq!(SAMPLES_PER_FRAME, 960);
    }
}
