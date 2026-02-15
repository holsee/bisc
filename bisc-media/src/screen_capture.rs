//! Screen and window capture using `scap`.
//!
//! Provides access to display and window enumeration, and produces
//! [`RawFrame`]s from a capture stream.

use anyhow::{Context, Result};
use tokio::sync::mpsc;

use crate::video_types::{PixelFormat, RawFrame};

/// Information about a capturable display.
#[derive(Debug, Clone)]
pub struct DisplayInfo {
    /// Display name/description.
    pub name: String,
    /// Unique display identifier.
    pub id: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Information about a capturable window.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Window title.
    pub title: String,
    /// Owning application name.
    pub app_name: String,
    /// Unique window identifier.
    pub id: u32,
}

/// Check if screen capture is supported on this platform.
pub fn is_supported() -> bool {
    scap::is_supported()
}

/// Check if screen capture permissions are granted.
pub fn has_permission() -> bool {
    scap::has_permission()
}

/// Request screen capture permissions (platform-specific).
pub fn request_permission() -> bool {
    scap::request_permission()
}

/// List available displays for capture.
pub fn list_displays() -> Vec<DisplayInfo> {
    let targets = scap::get_all_targets();

    targets
        .into_iter()
        .filter_map(|t| match t {
            scap::Target::Display(d) => Some(DisplayInfo {
                name: d.title.clone(),
                id: d.id,
                width: 0,
                height: 0,
            }),
            _ => None,
        })
        .collect()
}

/// List available windows for capture.
pub fn list_windows() -> Vec<WindowInfo> {
    let targets = scap::get_all_targets();

    targets
        .into_iter()
        .filter_map(|t| match t {
            scap::Target::Window(w) => Some(WindowInfo {
                title: w.title.clone(),
                app_name: String::new(),
                id: w.id,
            }),
            _ => None,
        })
        .collect()
}

/// Convert a scap Frame to a RawFrame.
fn frame_to_raw(frame: scap::frame::Frame) -> Option<RawFrame> {
    use scap::frame::VideoFrame;

    match frame {
        scap::frame::Frame::Video(video_frame) => match video_frame {
            VideoFrame::BGRA(f) => {
                let width = f.width as u32;
                let height = f.height as u32;
                let mut rgba = f.data;
                for chunk in rgba.chunks_exact_mut(4) {
                    chunk.swap(0, 2); // swap B and R
                }
                Some(RawFrame {
                    data: rgba,
                    width,
                    height,
                    format: PixelFormat::Rgba,
                })
            }
            VideoFrame::RGB(f) => {
                let width = f.width as u32;
                let height = f.height as u32;
                let mut rgba = Vec::with_capacity(f.data.len() / 3 * 4);
                for chunk in f.data.chunks_exact(3) {
                    rgba.push(chunk[0]);
                    rgba.push(chunk[1]);
                    rgba.push(chunk[2]);
                    rgba.push(255);
                }
                Some(RawFrame {
                    data: rgba,
                    width,
                    height,
                    format: PixelFormat::Rgba,
                })
            }
            VideoFrame::RGBx(f) => {
                let width = f.width as u32;
                let height = f.height as u32;
                Some(RawFrame {
                    data: f.data,
                    width,
                    height,
                    format: PixelFormat::Rgba,
                })
            }
            VideoFrame::BGRx(f) => {
                let width = f.width as u32;
                let height = f.height as u32;
                Some(RawFrame {
                    data: f.data,
                    width,
                    height,
                    format: PixelFormat::Rgba,
                })
            }
            _ => {
                tracing::warn!("unsupported video frame format from screen capture");
                None
            }
        },
        _ => {
            tracing::warn!("unsupported frame type from screen capture");
            None
        }
    }
}

/// Async stream of captured frames from a display or window.
pub struct ScreenCaptureStream {
    frame_rx: mpsc::UnboundedReceiver<RawFrame>,
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ScreenCaptureStream {
    /// Receive the next captured frame.
    pub async fn next_frame(&mut self) -> Option<RawFrame> {
        self.frame_rx.recv().await
    }

    /// Stop the capture stream and release resources.
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
            tracing::info!("screen capture stopped");
        }
    }
}

impl Drop for ScreenCaptureStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start capturing a display.
///
/// Returns a `ScreenCaptureStream` that yields `RawFrame`s.
pub fn capture_display(display_id: u32, fps: u32) -> Result<ScreenCaptureStream> {
    let targets = scap::get_all_targets();

    let target = targets
        .into_iter()
        .find(|t| matches!(t, scap::Target::Display(d) if d.id == display_id))
        .context("display not found")?;

    start_capture(Some(target), fps)
}

/// Start capturing a specific window.
///
/// Returns a `ScreenCaptureStream` that yields `RawFrame`s.
pub fn capture_window(window_id: u32, fps: u32) -> Result<ScreenCaptureStream> {
    let targets = scap::get_all_targets();

    let target = targets
        .into_iter()
        .find(|t| matches!(t, scap::Target::Window(w) if w.id == window_id))
        .context("window not found")?;

    start_capture(Some(target), fps)
}

/// Internal: start a capture with the given target and FPS.
fn start_capture(target: Option<scap::Target>, fps: u32) -> Result<ScreenCaptureStream> {
    if !scap::is_supported() {
        anyhow::bail!("screen capture is not supported on this platform");
    }
    if !scap::has_permission() {
        anyhow::bail!("screen capture permission not granted");
    }

    let options = scap::capturer::Options {
        fps,
        target,
        show_cursor: true,
        show_highlight: false,
        output_type: scap::frame::FrameType::BGRAFrame,
        ..Default::default()
    };

    let mut capturer =
        scap::capturer::Capturer::build(options).context("failed to build capturer")?;

    let (frame_tx, frame_rx) = mpsc::unbounded_channel();
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();

    // Spawn a blocking thread for the capture loop since scap is synchronous
    std::thread::spawn(move || {
        capturer.start_capture();
        tracing::info!("screen capture started");

        loop {
            // Check for stop signal
            if stop_rx.try_recv().is_ok() {
                break;
            }

            match capturer.get_next_frame() {
                Ok(frame) => {
                    if let Some(raw_frame) = frame_to_raw(frame) {
                        if frame_tx.send(raw_frame).is_err() {
                            break;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "screen capture frame error");
                    break;
                }
            }
        }

        capturer.stop_capture();
        tracing::info!("screen capture thread exited");
    });

    Ok(ScreenCaptureStream {
        frame_rx,
        stop_tx: Some(stop_tx),
    })
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
    fn list_displays_no_panic() {
        init_test_tracing();
        // Should not panic even without a display server
        let displays = list_displays();
        tracing::info!(count = displays.len(), "displays found");
    }

    #[test]
    fn list_windows_no_panic() {
        init_test_tracing();
        // Should not panic even without a display server
        let windows = list_windows();
        tracing::info!(count = windows.len(), "windows found");
    }

    #[test]
    fn is_supported_no_panic() {
        init_test_tracing();
        let supported = is_supported();
        tracing::info!(supported, "screen capture supported");
    }

    #[test]
    fn has_permission_no_panic() {
        init_test_tracing();
        let permitted = has_permission();
        tracing::info!(permitted, "screen capture permitted");
    }

    #[test]
    fn bgra_to_rgba_conversion() {
        // Test the channel swap logic
        let mut bgra = vec![
            0, 0, 255, 255, // Blue in BGRA -> Red in RGBA
            0, 255, 0, 255, // Green stays green
            255, 0, 0, 255, // Red in BGRA -> Blue in RGBA
        ];

        for chunk in bgra.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        assert_eq!(bgra[0], 255); // R
        assert_eq!(bgra[1], 0); // G
        assert_eq!(bgra[2], 0); // B
        assert_eq!(bgra[3], 255); // A

        assert_eq!(bgra[4], 0);
        assert_eq!(bgra[5], 255);
        assert_eq!(bgra[6], 0);

        assert_eq!(bgra[8], 0);
        assert_eq!(bgra[9], 0);
        assert_eq!(bgra[10], 255);
    }
}
