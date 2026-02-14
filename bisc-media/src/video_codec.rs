//! H.264 video encoding and decoding using `openh264`.
//!
//! Requires the `video-codec` feature to be enabled.

use anyhow::{Context, Result};
use openh264::decoder::{Decoder, DecoderConfig};
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, RateControlMode};
use openh264::formats::{YUVSlices, YUVSource};
use openh264::nal_units;
use openh264::OpenH264API;

use crate::video_types::{PixelFormat, RawFrame};

/// Convert RGBA pixel data to I420 (YUV 4:2:0 planar).
///
/// Width and height must be even.
pub fn rgba_to_i420(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_size = (w / 2) * (h / 2);
    let mut yuv = vec![0u8; y_size + uv_size * 2];

    let (y_plane, uv_planes) = yuv.split_at_mut(y_size);
    let (u_plane, v_plane) = uv_planes.split_at_mut(uv_size);

    // Convert each pixel to Y
    for row in 0..h {
        for col in 0..w {
            let idx = (row * w + col) * 4;
            let r = rgba[idx] as f32;
            let g = rgba[idx + 1] as f32;
            let b = rgba[idx + 2] as f32;
            y_plane[row * w + col] = (0.299 * r + 0.587 * g + 0.114 * b)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }

    // Subsample U and V (2x2 blocks)
    for row in (0..h).step_by(2) {
        for col in (0..w).step_by(2) {
            let mut r_sum = 0.0f32;
            let mut g_sum = 0.0f32;
            let mut b_sum = 0.0f32;
            for dr in 0..2 {
                for dc in 0..2 {
                    let idx = ((row + dr) * w + (col + dc)) * 4;
                    r_sum += rgba[idx] as f32;
                    g_sum += rgba[idx + 1] as f32;
                    b_sum += rgba[idx + 2] as f32;
                }
            }
            let r = r_sum / 4.0;
            let g = g_sum / 4.0;
            let b = b_sum / 4.0;

            let uv_idx = (row / 2) * (w / 2) + (col / 2);
            u_plane[uv_idx] = (-0.169 * r - 0.331 * g + 0.500 * b + 128.0)
                .round()
                .clamp(0.0, 255.0) as u8;
            v_plane[uv_idx] = (0.500 * r - 0.419 * g - 0.081 * b + 128.0)
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }

    yuv
}

/// Convert I420 (YUV 4:2:0 planar) to RGBA pixel data.
pub fn i420_to_rgba(yuv: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_w = w / 2;

    let y_plane = &yuv[..y_size];
    let u_plane = &yuv[y_size..y_size + (uv_w * h / 2)];
    let v_plane = &yuv[y_size + (uv_w * h / 2)..];

    let mut rgba = vec![255u8; w * h * 4];

    for row in 0..h {
        for col in 0..w {
            let y = y_plane[row * w + col] as f32;
            let uv_idx = (row / 2) * uv_w + (col / 2);
            let u = u_plane[uv_idx] as f32 - 128.0;
            let v = v_plane[uv_idx] as f32 - 128.0;

            let r = (y + 1.402 * v).round().clamp(0.0, 255.0) as u8;
            let g = (y - 0.344 * u - 0.714 * v).round().clamp(0.0, 255.0) as u8;
            let b = (y + 1.772 * u).round().clamp(0.0, 255.0) as u8;

            let out_idx = (row * w + col) * 4;
            rgba[out_idx] = r;
            rgba[out_idx + 1] = g;
            rgba[out_idx + 2] = b;
            // alpha already 255
        }
    }

    rgba
}

/// H.264 video encoder wrapping OpenH264.
pub struct VideoEncoder {
    encoder: Encoder,
    width: u32,
    height: u32,
}

impl VideoEncoder {
    /// Create a new video encoder.
    ///
    /// `width` and `height` must be even. `framerate` in fps, `bitrate` in bps.
    pub fn new(width: u32, height: u32, framerate: u32, bitrate: u32) -> Result<Self> {
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(bitrate))
            .max_frame_rate(FrameRate::from_hz(framerate as f32))
            .rate_control_mode(RateControlMode::Bitrate);

        let api = OpenH264API::from_source();
        let encoder = Encoder::with_api_config(api, config).context("failed to create encoder")?;

        tracing::info!(width, height, framerate, bitrate, "video encoder created");

        Ok(Self {
            encoder,
            width,
            height,
        })
    }

    /// Encode a raw frame, returning NAL units.
    ///
    /// Input frame must match the encoder's width/height. If the frame is RGBA,
    /// it will be converted to I420 before encoding.
    pub fn encode(&mut self, frame: &RawFrame) -> Result<Vec<Vec<u8>>> {
        anyhow::ensure!(
            frame.width == self.width && frame.height == self.height,
            "frame dimensions {}x{} don't match encoder {}x{}",
            frame.width,
            frame.height,
            self.width,
            self.height
        );

        let i420_data;
        let yuv_data = match frame.format {
            PixelFormat::Rgba => {
                i420_data = rgba_to_i420(&frame.data, self.width, self.height);
                &i420_data
            }
            PixelFormat::I420 => &frame.data,
            PixelFormat::Nv12 => {
                anyhow::bail!("NV12 input not supported; convert to RGBA or I420 first");
            }
        };

        let w = self.width as usize;
        let h = self.height as usize;
        let y_size = w * h;
        let uv_size = (w / 2) * (h / 2);

        let y_plane = &yuv_data[..y_size];
        let u_plane = &yuv_data[y_size..y_size + uv_size];
        let v_plane = &yuv_data[y_size + uv_size..y_size + uv_size * 2];

        let yuv = YUVSlices::new((y_plane, u_plane, v_plane), (w, h), (w, w / 2, w / 2));

        let bitstream = self.encoder.encode(&yuv).context("encode failed")?;

        // Get the full bitstream (includes SPS/PPS + video NALs) then split
        let full_bytes = bitstream.to_vec();
        let result: Vec<Vec<u8>> = nal_units(&full_bytes).map(|n| n.to_vec()).collect();

        tracing::trace!(
            nal_count = result.len(),
            total_bytes = result.iter().map(|n| n.len()).sum::<usize>(),
            "encoded frame"
        );

        Ok(result)
    }

    /// Set the target bitrate at runtime (in bps).
    pub fn set_bitrate(&mut self, bps: u32) {
        // ENCODER_OPTION_BITRATE = 5 from the OpenH264 C API (codec_app_def.h)
        const ENCODER_OPTION_BITRATE: std::ffi::c_int = 5;

        unsafe {
            let raw = self.encoder.raw_api();
            let mut bitrate = bps as std::ffi::c_int;
            raw.set_option(
                ENCODER_OPTION_BITRATE,
                std::ptr::addr_of_mut!(bitrate).cast(),
            );
        }

        tracing::debug!(bps, "set encoder bitrate");
    }

    /// Force the next encoded frame to be an IDR keyframe.
    pub fn force_keyframe(&mut self) {
        self.encoder.force_intra_frame();
        tracing::debug!("forced keyframe");
    }
}

/// H.264 video decoder wrapping OpenH264.
pub struct VideoDecoder {
    decoder: Decoder,
}

impl VideoDecoder {
    /// Create a new video decoder.
    pub fn new() -> Result<Self> {
        let api = OpenH264API::from_source();
        let decoder = Decoder::with_api_config(api, DecoderConfig::new())
            .context("failed to create decoder")?;

        tracing::info!("video decoder created");

        Ok(Self { decoder })
    }

    /// Decode H.264 NAL units into a raw frame.
    ///
    /// Returns `None` if the NAL data was consumed but no frame was produced
    /// (e.g., SPS/PPS parameter sets).
    pub fn decode(&mut self, nal_data: &[u8]) -> Result<Option<RawFrame>> {
        let decoded = self.decoder.decode(nal_data).context("decode failed")?;

        match decoded {
            Some(yuv) => {
                let (w, h) = yuv.dimensions();
                let mut rgba = vec![0u8; w * h * 4];
                yuv.write_rgba8(&mut rgba);

                tracing::trace!(width = w, height = h, "decoded frame");

                Ok(Some(RawFrame {
                    data: rgba,
                    width: w as u32,
                    height: h as u32,
                    format: PixelFormat::Rgba,
                }))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_test_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "debug".into()),
            )
            .with_test_writer()
            .try_init();
    }

    /// Create a solid-color RGBA test frame.
    fn solid_rgba_frame(width: u32, height: u32, r: u8, g: u8, b: u8) -> RawFrame {
        let pixel_count = (width * height) as usize;
        let mut data = Vec::with_capacity(pixel_count * 4);
        for _ in 0..pixel_count {
            data.push(r);
            data.push(g);
            data.push(b);
            data.push(255);
        }
        RawFrame {
            data,
            width,
            height,
            format: PixelFormat::Rgba,
        }
    }

    #[test]
    fn encode_synthetic_frame_produces_valid_nal_units() {
        init_test_tracing();

        let mut encoder = VideoEncoder::new(640, 480, 30, 500_000).unwrap();
        let frame = solid_rgba_frame(640, 480, 128, 64, 32);

        let nal_units = encoder.encode(&frame).unwrap();
        assert!(
            !nal_units.is_empty(),
            "should produce at least one NAL unit"
        );

        let total_bytes: usize = nal_units.iter().map(|n| n.len()).sum();
        assert!(total_bytes > 0, "NAL data should be non-empty");

        tracing::info!(
            nal_count = nal_units.len(),
            total_bytes,
            "encoded first frame"
        );
    }

    #[test]
    fn decode_encoded_frame_matches_dimensions() {
        init_test_tracing();

        let mut encoder = VideoEncoder::new(320, 240, 30, 500_000).unwrap();
        let mut decoder = VideoDecoder::new().unwrap();

        let frame = solid_rgba_frame(320, 240, 0, 255, 0);
        let nal_units = encoder.encode(&frame).unwrap();

        // Feed all NAL units to the decoder
        let mut decoded_frame = None;
        for nal in &nal_units {
            if let Some(f) = decoder.decode(nal).unwrap() {
                decoded_frame = Some(f);
            }
        }

        let decoded = decoded_frame.expect("should decode at least one frame");
        assert_eq!(decoded.width, 320);
        assert_eq!(decoded.height, 240);
        assert_eq!(decoded.format, PixelFormat::Rgba);
        assert_eq!(decoded.data.len(), 320 * 240 * 4);

        tracing::info!(
            width = decoded.width,
            height = decoded.height,
            "decoded frame dimensions match"
        );
    }

    #[test]
    fn first_frame_is_keyframe() {
        init_test_tracing();

        let mut encoder = VideoEncoder::new(160, 120, 30, 200_000).unwrap();
        let frame = solid_rgba_frame(160, 120, 255, 0, 0);

        // OpenH264 always produces an IDR for the first frame.
        // We verify by decoding — if it's not a keyframe, the decoder would fail.
        let nal_units = encoder.encode(&frame).unwrap();

        let mut decoder = VideoDecoder::new().unwrap();
        let mut decoded = false;
        for nal in &nal_units {
            if decoder.decode(nal).unwrap().is_some() {
                decoded = true;
            }
        }
        assert!(decoded, "first frame should be decodable (IDR keyframe)");
    }

    #[test]
    fn force_keyframe_produces_idr() {
        init_test_tracing();

        let mut encoder = VideoEncoder::new(160, 120, 30, 200_000).unwrap();

        // Encode several P-frames first
        for i in 0..10 {
            let frame = solid_rgba_frame(160, 120, (i * 20) as u8, 100, 50);
            encoder.encode(&frame).unwrap();
        }

        // Force keyframe
        encoder.force_keyframe();
        let frame = solid_rgba_frame(160, 120, 255, 255, 0);
        let nal_units = encoder.encode(&frame).unwrap();

        // Verify the forced keyframe can be decoded independently
        let mut decoder = VideoDecoder::new().unwrap();
        let mut decoded = false;
        for nal in &nal_units {
            if decoder.decode(nal).unwrap().is_some() {
                decoded = true;
            }
        }
        assert!(decoded, "forced keyframe should be independently decodable");
    }

    #[test]
    fn bitrate_change_affects_encoded_size() {
        init_test_tracing();

        // Encode frames at high bitrate
        let mut encoder_high = VideoEncoder::new(320, 240, 30, 2_000_000).unwrap();
        let mut total_high = 0usize;
        for i in 0..30 {
            let frame = solid_rgba_frame(320, 240, (i * 8) as u8, (i * 4) as u8, i as u8);
            let nals = encoder_high.encode(&frame).unwrap();
            total_high += nals.iter().map(|n| n.len()).sum::<usize>();
        }

        // Encode same frames at low bitrate
        let mut encoder_low = VideoEncoder::new(320, 240, 30, 100_000).unwrap();
        let mut total_low = 0usize;
        for i in 0..30 {
            let frame = solid_rgba_frame(320, 240, (i * 8) as u8, (i * 4) as u8, i as u8);
            let nals = encoder_low.encode(&frame).unwrap();
            total_low += nals.iter().map(|n| n.len()).sum::<usize>();
        }

        tracing::info!(total_high, total_low, "bitrate comparison: high vs low");
        assert!(
            total_low < total_high,
            "lower bitrate ({total_low} bytes) should produce smaller output than higher bitrate ({total_high} bytes)"
        );
    }

    #[test]
    fn set_bitrate_at_runtime() {
        init_test_tracing();

        let mut encoder = VideoEncoder::new(160, 120, 30, 500_000).unwrap();

        // Encode a few frames at original bitrate
        for i in 0..5 {
            let frame = solid_rgba_frame(160, 120, i * 40, 100, 50);
            encoder.encode(&frame).unwrap();
        }

        // Change bitrate at runtime — should not panic
        encoder.set_bitrate(100_000);

        for i in 0..5 {
            let frame = solid_rgba_frame(160, 120, i * 40, 100, 50);
            encoder.encode(&frame).unwrap();
        }
    }

    #[test]
    fn rgba_i420_roundtrip_uniform() {
        // Uniform color: no chroma subsampling loss expected
        let width = 16u32;
        let height = 16u32;
        let (r, g, b) = (100u8, 150u8, 200u8);

        let mut rgba = vec![0u8; (width * height * 4) as usize];
        for i in 0..(width * height) as usize {
            rgba[i * 4] = r;
            rgba[i * 4 + 1] = g;
            rgba[i * 4 + 2] = b;
            rgba[i * 4 + 3] = 255;
        }

        let i420 = rgba_to_i420(&rgba, width, height);
        let expected_size = (width * height * 3 / 2) as usize;
        assert_eq!(i420.len(), expected_size);

        let roundtrip = i420_to_rgba(&i420, width, height);
        assert_eq!(roundtrip.len(), (width * height * 4) as usize);

        // Uniform color: only 8-bit truncation error, max ~2 per channel
        for i in 0..(width * height) as usize {
            let dr = (rgba[i * 4] as i32 - roundtrip[i * 4] as i32).unsigned_abs();
            let dg = (rgba[i * 4 + 1] as i32 - roundtrip[i * 4 + 1] as i32).unsigned_abs();
            let db = (rgba[i * 4 + 2] as i32 - roundtrip[i * 4 + 2] as i32).unsigned_abs();
            assert!(
                dr <= 2 && dg <= 2 && db <= 2,
                "pixel {i} too different: orig=({r},{g},{b}) roundtrip=({},{},{})",
                roundtrip[i * 4],
                roundtrip[i * 4 + 1],
                roundtrip[i * 4 + 2]
            );
        }
    }

    #[test]
    fn rgba_i420_correct_sizes() {
        let width = 320u32;
        let height = 240u32;
        let rgba = vec![128u8; (width * height * 4) as usize];

        let i420 = rgba_to_i420(&rgba, width, height);
        assert_eq!(i420.len(), (width * height * 3 / 2) as usize);

        let back = i420_to_rgba(&i420, width, height);
        assert_eq!(back.len(), (width * height * 4) as usize);
    }
}
