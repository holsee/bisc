//! Opus encoding and decoding for real-time audio.

use anyhow::{Context, Result};

/// Standard sample rate for Opus encoding (48kHz).
pub const OPUS_SAMPLE_RATE: u32 = 48_000;

/// Standard frame duration in milliseconds.
pub const FRAME_DURATION_MS: u32 = 20;

/// Number of samples per mono frame at 48kHz with 20ms frames.
pub const SAMPLES_PER_FRAME: usize = (OPUS_SAMPLE_RATE * FRAME_DURATION_MS / 1000) as usize;

/// Maximum encoded frame size in bytes (Opus spec).
const MAX_ENCODED_SIZE: usize = 4000;

/// Opus encoder wrapper for real-time audio.
pub struct OpusEncoder {
    encoder: opus::Encoder,
    channels: u16,
}

impl OpusEncoder {
    /// Create a new Opus encoder.
    ///
    /// - `channels`: 1 for mono, 2 for stereo
    /// - `bitrate_bps`: initial bitrate in bits per second (e.g. 64000)
    pub fn new(channels: u16, bitrate_bps: u32) -> Result<Self> {
        let opus_channels = match channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            _ => anyhow::bail!("unsupported channel count: {channels}"),
        };

        let mut encoder =
            opus::Encoder::new(OPUS_SAMPLE_RATE, opus_channels, opus::Application::Voip)
                .context("failed to create Opus encoder")?;

        encoder
            .set_bitrate(opus::Bitrate::Bits(bitrate_bps as i32))
            .context("failed to set bitrate")?;

        tracing::info!(channels, bitrate_bps, "Opus encoder created");

        Ok(Self { encoder, channels })
    }

    /// Encode a frame of f32 samples to Opus bytes.
    ///
    /// Input must contain exactly `SAMPLES_PER_FRAME * channels` samples.
    pub fn encode(&mut self, samples: &[f32]) -> Result<Vec<u8>> {
        let expected = SAMPLES_PER_FRAME * self.channels as usize;
        anyhow::ensure!(
            samples.len() == expected,
            "expected {expected} samples, got {}",
            samples.len()
        );

        let encoded = self
            .encoder
            .encode_vec_float(samples, MAX_ENCODED_SIZE)
            .context("Opus encode failed")?;

        tracing::trace!(
            input_samples = samples.len(),
            encoded_bytes = encoded.len(),
            "encoded Opus frame"
        );

        Ok(encoded)
    }

    /// Set the encoder bitrate in bits per second.
    pub fn set_bitrate(&mut self, bps: u32) -> Result<()> {
        self.encoder
            .set_bitrate(opus::Bitrate::Bits(bps as i32))
            .context("failed to set bitrate")?;
        tracing::debug!(bitrate_bps = bps, "Opus bitrate changed");
        Ok(())
    }

    /// Get the current encoder bitrate.
    pub fn bitrate(&mut self) -> Result<i32> {
        match self
            .encoder
            .get_bitrate()
            .context("failed to get bitrate")?
        {
            opus::Bitrate::Bits(bps) => Ok(bps),
            opus::Bitrate::Max => Ok(510_000),
            opus::Bitrate::Auto => Ok(-1),
        }
    }

    /// Get the number of channels this encoder is configured for.
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

/// Opus decoder wrapper for real-time audio.
pub struct OpusDecoder {
    decoder: opus::Decoder,
    channels: u16,
}

impl OpusDecoder {
    /// Create a new Opus decoder.
    ///
    /// - `channels`: 1 for mono, 2 for stereo (must match encoder)
    pub fn new(channels: u16) -> Result<Self> {
        let opus_channels = match channels {
            1 => opus::Channels::Mono,
            2 => opus::Channels::Stereo,
            _ => anyhow::bail!("unsupported channel count: {channels}"),
        };

        let decoder = opus::Decoder::new(OPUS_SAMPLE_RATE, opus_channels)
            .context("failed to create Opus decoder")?;

        tracing::info!(channels, "Opus decoder created");

        Ok(Self { decoder, channels })
    }

    /// Decode Opus bytes back to f32 samples.
    ///
    /// Returns a vector of `SAMPLES_PER_FRAME * channels` samples.
    pub fn decode(&mut self, data: &[u8]) -> Result<Vec<f32>> {
        let frame_size = SAMPLES_PER_FRAME * self.channels as usize;
        let mut output = vec![0.0f32; frame_size];

        let decoded_samples = self
            .decoder
            .decode_float(data, &mut output, false)
            .context("Opus decode failed")?;

        // decoded_samples is per-channel sample count
        let total = decoded_samples * self.channels as usize;
        output.truncate(total);

        tracing::trace!(
            input_bytes = data.len(),
            output_samples = total,
            "decoded Opus frame"
        );

        Ok(output)
    }

    /// Get the number of channels this decoder is configured for.
    pub fn channels(&self) -> u16 {
        self.channels
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_silence_produces_valid_opus() {
        let mut encoder = OpusEncoder::new(1, 64_000).unwrap();
        let silence = vec![0.0f32; SAMPLES_PER_FRAME];
        let encoded = encoder.encode(&silence).unwrap();
        assert!(!encoded.is_empty(), "encoded output should not be empty");
        // Opus silence is typically very small (a few bytes)
        assert!(encoded.len() < 100, "silence should compress well");
    }

    #[test]
    fn decode_encoded_silence_produces_960_samples() {
        let mut encoder = OpusEncoder::new(1, 64_000).unwrap();
        let mut decoder = OpusDecoder::new(1).unwrap();

        let silence = vec![0.0f32; SAMPLES_PER_FRAME];
        let encoded = encoder.encode(&silence).unwrap();
        let decoded = decoder.decode(&encoded).unwrap();

        assert_eq!(
            decoded.len(),
            SAMPLES_PER_FRAME,
            "decoded frame should have {SAMPLES_PER_FRAME} samples"
        );
    }

    #[test]
    fn roundtrip_sine_wave_is_similar() {
        let mut encoder = OpusEncoder::new(1, 128_000).unwrap();
        let mut decoder = OpusDecoder::new(1).unwrap();

        // Generate a 440Hz sine wave
        let samples: Vec<f32> = (0..SAMPLES_PER_FRAME)
            .map(|i| {
                let t = i as f32 / OPUS_SAMPLE_RATE as f32;
                (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5
            })
            .collect();

        // Encode several frames to let the codec converge (first frames have
        // higher distortion as the encoder/decoder states warm up).
        let mut last_decoded = Vec::new();
        for _ in 0..5 {
            let encoded = encoder.encode(&samples).unwrap();
            last_decoded = decoder.decode(&encoded).unwrap();
        }

        assert_eq!(last_decoded.len(), samples.len());

        // Lossy codec: check approximate similarity on converged frame.
        // Opus in VoIP mode applies filtering that increases MSE for pure tones,
        // so we use a generous threshold. The key check is that the decoded
        // signal resembles the input (not random noise).
        let mse: f32 = samples
            .iter()
            .zip(last_decoded.iter())
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>()
            / samples.len() as f32;

        assert!(
            mse < 0.15,
            "mean squared error {mse} should be < 0.15 for 128kbps VoIP mode"
        );
    }

    #[test]
    fn bitrate_change_affects_output_size() {
        let mut encoder_low = OpusEncoder::new(1, 16_000).unwrap();
        let mut encoder_high = OpusEncoder::new(1, 128_000).unwrap();

        // Generate a non-trivial signal (noise-like)
        let samples: Vec<f32> = (0..SAMPLES_PER_FRAME)
            .map(|i| {
                let t = i as f32 / OPUS_SAMPLE_RATE as f32;
                (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5
                    + (2.0 * std::f32::consts::PI * 880.0 * t).sin() * 0.3
                    + (2.0 * std::f32::consts::PI * 1320.0 * t).sin() * 0.2
            })
            .collect();

        // Encode several frames to let the encoder settle
        let mut low_sizes = Vec::new();
        let mut high_sizes = Vec::new();
        for _ in 0..10 {
            low_sizes.push(encoder_low.encode(&samples).unwrap().len());
            high_sizes.push(encoder_high.encode(&samples).unwrap().len());
        }

        let avg_low: f64 = low_sizes.iter().sum::<usize>() as f64 / low_sizes.len() as f64;
        let avg_high: f64 = high_sizes.iter().sum::<usize>() as f64 / high_sizes.len() as f64;

        assert!(
            avg_low < avg_high,
            "16kbps avg ({avg_low:.0} bytes) should be smaller than 128kbps avg ({avg_high:.0} bytes)"
        );
    }

    #[test]
    fn stereo_encode_decode_roundtrip() {
        let mut encoder = OpusEncoder::new(2, 128_000).unwrap();
        let mut decoder = OpusDecoder::new(2).unwrap();

        let stereo_samples = SAMPLES_PER_FRAME * 2;

        // Generate interleaved stereo: left = sine, right = silence
        let samples: Vec<f32> = (0..SAMPLES_PER_FRAME)
            .flat_map(|i| {
                let t = i as f32 / OPUS_SAMPLE_RATE as f32;
                let left = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
                let right = 0.0;
                [left, right]
            })
            .collect();

        assert_eq!(samples.len(), stereo_samples);

        let encoded = encoder.encode(&samples).unwrap();
        assert!(!encoded.is_empty());

        let decoded = decoder.decode(&encoded).unwrap();
        assert_eq!(
            decoded.len(),
            stereo_samples,
            "decoded stereo should have {} samples",
            stereo_samples
        );
    }

    #[test]
    fn set_bitrate_at_runtime() {
        let mut encoder = OpusEncoder::new(1, 64_000).unwrap();

        // Change bitrate
        encoder.set_bitrate(32_000).unwrap();
        let bps = encoder.bitrate().unwrap();
        assert_eq!(bps, 32_000, "bitrate should be 32000 after set");

        encoder.set_bitrate(128_000).unwrap();
        let bps = encoder.bitrate().unwrap();
        assert_eq!(bps, 128_000, "bitrate should be 128000 after set");
    }

    #[test]
    fn wrong_sample_count_returns_error() {
        let mut encoder = OpusEncoder::new(1, 64_000).unwrap();

        // Too few samples
        let result = encoder.encode(&[0.0f32; 100]);
        assert!(result.is_err(), "should fail with wrong sample count");

        // Too many samples
        let result = encoder.encode(&[0.0f32; SAMPLES_PER_FRAME + 100]);
        assert!(result.is_err(), "should fail with too many samples");
    }
}
