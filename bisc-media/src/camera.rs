//! Camera capture using `nokhwa`.
//!
//! Requires the `video` feature to be enabled.

use anyhow::{Context, Result};
use nokhwa::pixel_format::RgbAFormat;
use nokhwa::utils::{
    ApiBackend, CameraFormat, CameraIndex, RequestedFormat, RequestedFormatType, Resolution,
};

pub use crate::video_types::{PixelFormat, RawFrame};

/// Camera configuration.
#[derive(Debug, Clone)]
pub struct CameraConfig {
    /// Horizontal resolution in pixels.
    pub width: u32,
    /// Vertical resolution in pixels.
    pub height: u32,
    /// Target frame rate.
    pub frame_rate: u32,
    /// Desired pixel format for captured frames.
    pub pixel_format: PixelFormat,
}

impl Default for CameraConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            frame_rate: 30,
            pixel_format: PixelFormat::Rgba,
        }
    }
}

/// Information about an available camera.
#[derive(Debug, Clone)]
pub struct CameraInfo {
    /// Human-readable camera name.
    pub name: String,
    /// Camera index (for opening).
    pub index: u32,
    /// Additional description (backend-specific).
    pub description: String,
}

/// List available cameras.
///
/// Returns an empty list if no cameras are detected (does not panic).
pub fn list_cameras() -> Result<Vec<CameraInfo>> {
    let cameras = match nokhwa::query(ApiBackend::Auto) {
        Ok(c) => c,
        Err(e) => {
            tracing::debug!(error = %e, "camera query returned error, treating as empty");
            return Ok(Vec::new());
        }
    };

    let mut result = Vec::new();
    for cam in cameras {
        let index = match cam.index() {
            CameraIndex::Index(i) => *i,
            CameraIndex::String(_) => continue,
        };
        result.push(CameraInfo {
            name: cam.human_name().to_string(),
            index,
            description: cam.description().to_string(),
        });
    }

    tracing::debug!(count = result.len(), "enumerated cameras");
    Ok(result)
}

/// Camera wrapping `nokhwa` for video frame capture.
pub struct Camera {
    inner: nokhwa::Camera,
    config: CameraConfig,
}

impl Camera {
    /// Open a camera with the given configuration.
    pub fn open(camera_index: u32, config: CameraConfig) -> Result<Self> {
        let index = CameraIndex::Index(camera_index);

        let requested_format =
            RequestedFormat::new::<RgbAFormat>(RequestedFormatType::Closest(CameraFormat::new(
                Resolution::new(config.width, config.height),
                nokhwa::utils::FrameFormat::MJPEG,
                config.frame_rate,
            )));

        let mut camera =
            nokhwa::Camera::new(index, requested_format).context("failed to open camera")?;

        camera
            .open_stream()
            .context("failed to open camera stream")?;

        let actual_res = camera.resolution();
        tracing::info!(
            camera_index,
            width = actual_res.width(),
            height = actual_res.height(),
            fps = camera.frame_rate(),
            "camera opened"
        );

        let config = CameraConfig {
            width: actual_res.width(),
            height: actual_res.height(),
            frame_rate: camera.frame_rate(),
            ..config
        };

        Ok(Self {
            inner: camera,
            config,
        })
    }

    /// Capture a single frame.
    pub fn capture_frame(&mut self) -> Result<RawFrame> {
        match self.config.pixel_format {
            PixelFormat::Rgba => {
                let res = self.inner.resolution();
                let buf_size = (res.width() * res.height() * 4) as usize;
                let mut data = vec![0u8; buf_size];
                self.inner
                    .write_frame_to_buffer::<RgbAFormat>(&mut data)
                    .context("failed to capture and decode frame")?;
                Ok(RawFrame {
                    data,
                    width: res.width(),
                    height: res.height(),
                    format: PixelFormat::Rgba,
                })
            }
            PixelFormat::Nv12 => {
                let frame = self.inner.frame().context("failed to capture frame")?;
                let res = frame.resolution();
                Ok(RawFrame {
                    data: frame.buffer().to_vec(),
                    width: res.width(),
                    height: res.height(),
                    format: PixelFormat::Nv12,
                })
            }
        }
    }

    /// Stop the camera stream.
    pub fn stop(&mut self) -> Result<()> {
        self.inner
            .stop_stream()
            .context("failed to stop camera stream")?;
        tracing::info!("camera stopped");
        Ok(())
    }

    /// Restart the camera stream after stopping.
    pub fn restart(&mut self) -> Result<()> {
        self.inner
            .open_stream()
            .context("failed to restart camera stream")?;
        tracing::info!("camera restarted");
        Ok(())
    }

    /// Get the current configuration (actual negotiated values).
    pub fn config(&self) -> &CameraConfig {
        &self.config
    }

    /// Check if the stream is currently open.
    pub fn is_open(&self) -> bool {
        self.inner.is_stream_open()
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

    #[test]
    fn list_cameras_does_not_panic() {
        init_test_tracing();

        match list_cameras() {
            Ok(cameras) => {
                tracing::info!(count = cameras.len(), "found cameras");
                for cam in &cameras {
                    tracing::info!(index = cam.index, name = %cam.name, "camera");
                }
            }
            Err(e) => {
                tracing::info!(error = %e, "could not enumerate cameras");
            }
        }
    }

    #[test]
    fn camera_config_default() {
        let config = CameraConfig::default();
        assert_eq!(config.width, 640);
        assert_eq!(config.height, 480);
        assert_eq!(config.frame_rate, 30);
        assert_eq!(config.pixel_format, PixelFormat::Rgba);
    }

    #[test]
    fn raw_frame_fields() {
        let frame = RawFrame {
            data: vec![0u8; 640 * 480 * 4],
            width: 640,
            height: 480,
            format: PixelFormat::Rgba,
        };
        assert_eq!(frame.width, 640);
        assert_eq!(frame.height, 480);
        assert_eq!(frame.format, PixelFormat::Rgba);
        assert_eq!(frame.data.len(), 640 * 480 * 4);
    }

    #[test]
    fn open_camera_and_capture_frame() {
        init_test_tracing();

        let cameras = match list_cameras() {
            Ok(c) => c,
            Err(e) => {
                tracing::info!(error = %e, "skipping: cannot enumerate cameras");
                return;
            }
        };

        if cameras.is_empty() {
            tracing::info!("skipping: no cameras available");
            return;
        }

        let cam_info = &cameras[0];
        let config = CameraConfig::default();

        let mut camera = match Camera::open(cam_info.index, config) {
            Ok(c) => c,
            Err(e) => {
                tracing::info!(error = %e, "skipping: failed to open camera");
                return;
            }
        };

        assert!(camera.is_open());

        // Capture a frame and verify dimensions match config
        let frame = camera.capture_frame().expect("failed to capture frame");
        assert_eq!(frame.width, camera.config().width);
        assert_eq!(frame.height, camera.config().height);
        assert_eq!(frame.format, PixelFormat::Rgba);
        assert_eq!(frame.data.len(), (frame.width * frame.height * 4) as usize);

        camera.stop().expect("failed to stop camera");
    }

    #[test]
    fn camera_stop_and_restart() {
        init_test_tracing();

        let cameras = match list_cameras() {
            Ok(c) => c,
            Err(e) => {
                tracing::info!(error = %e, "skipping: cannot enumerate cameras");
                return;
            }
        };

        if cameras.is_empty() {
            tracing::info!("skipping: no cameras available");
            return;
        }

        let cam_info = &cameras[0];
        let config = CameraConfig::default();

        let mut camera = match Camera::open(cam_info.index, config) {
            Ok(c) => c,
            Err(e) => {
                tracing::info!(error = %e, "skipping: failed to open camera");
                return;
            }
        };

        assert!(camera.is_open());

        camera.stop().expect("failed to stop");
        assert!(!camera.is_open());

        camera.restart().expect("failed to restart");
        assert!(camera.is_open());

        // Verify we can still capture after restart
        let frame = camera
            .capture_frame()
            .expect("failed to capture after restart");
        assert_eq!(frame.width, camera.config().width);
        assert_eq!(frame.height, camera.config().height);

        camera.stop().expect("failed to stop camera");
    }
}
