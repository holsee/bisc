//! Shared types for video capture and encoding/decoding.

/// Pixel format of a raw frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// RGBA 8-bit per channel (4 bytes per pixel).
    Rgba,
    /// NV12 (YUV 4:2:0 semi-planar).
    Nv12,
    /// I420 / YUV420P (YUV 4:2:0 planar — Y, U, V separate planes).
    I420,
}

/// A raw video frame.
#[derive(Debug, Clone)]
pub struct RawFrame {
    /// Pixel data.
    pub data: Vec<u8>,
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Pixel format.
    pub format: PixelFormat,
}
