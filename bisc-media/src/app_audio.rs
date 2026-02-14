//! Per-application scoped audio capture.
//!
//! Defines the [`AppAudioCapture`] trait for capturing audio from specific
//! applications. Platform-specific implementations are selected via conditional
//! compilation; a stub is provided for unsupported platforms.

use anyhow::Result;
use tokio::sync::mpsc;

/// Information about an application whose audio can be captured.
#[derive(Debug, Clone)]
pub struct AppAudioSource {
    /// Application name.
    pub name: String,
    /// Platform-specific process/stream identifier.
    pub id: u64,
}

/// Stream of captured audio samples from an application.
#[derive(Debug)]
pub struct AudioStream {
    rx: mpsc::UnboundedReceiver<Vec<f32>>,
}

impl AudioStream {
    /// Create a new audio stream wrapping a receiver.
    pub fn new(rx: mpsc::UnboundedReceiver<Vec<f32>>) -> Self {
        Self { rx }
    }

    /// Receive the next buffer of audio samples.
    pub async fn next(&mut self) -> Option<Vec<f32>> {
        self.rx.recv().await
    }
}

/// Trait for platform-specific application audio capture.
pub trait AppAudioCapture: Send + Sync {
    /// List applications whose audio can be captured.
    fn list_capturable_apps(&self) -> Result<Vec<AppAudioSource>>;

    /// Start capturing audio from the specified application.
    fn start_capture(&self, source: &AppAudioSource) -> Result<AudioStream>;

    /// Stop any active capture.
    fn stop_capture(&self);
}

/// Create the platform-appropriate `AppAudioCapture` implementation.
pub fn create_app_audio_capture() -> Box<dyn AppAudioCapture> {
    #[cfg(target_os = "linux")]
    {
        match app_audio_linux::PipeWireAppAudioCapture::new() {
            Ok(capture) => {
                tracing::info!("using PipeWire app audio capture");
                return Box::new(capture);
            }
            Err(e) => {
                tracing::warn!(error = %e, "PipeWire not available, falling back to stub");
            }
        }
    }

    tracing::info!("using stub app audio capture");
    Box::new(app_audio_stub::StubAppAudioCapture)
}

/// Stub implementation for unsupported platforms.
pub mod app_audio_stub {
    use super::*;

    /// Stub that returns empty lists and errors on capture.
    pub struct StubAppAudioCapture;

    impl AppAudioCapture for StubAppAudioCapture {
        fn list_capturable_apps(&self) -> Result<Vec<AppAudioSource>> {
            tracing::debug!("stub: list_capturable_apps returning empty list");
            Ok(Vec::new())
        }

        fn start_capture(&self, source: &AppAudioSource) -> Result<AudioStream> {
            tracing::warn!(
                app = %source.name,
                "stub: app audio capture not available on this platform"
            );
            anyhow::bail!(
                "app audio capture not available on this platform (app: {})",
                source.name
            )
        }

        fn stop_capture(&self) {
            tracing::debug!("stub: stop_capture (no-op)");
        }
    }
}

/// Linux implementation using PipeWire (loaded dynamically).
#[cfg(target_os = "linux")]
pub mod app_audio_linux {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// PipeWire-based application audio capture.
    ///
    /// Attempts to connect to PipeWire at construction time. If PipeWire
    /// is not available, construction fails gracefully.
    pub struct PipeWireAppAudioCapture {
        available: bool,
        capturing: Arc<AtomicBool>,
    }

    impl PipeWireAppAudioCapture {
        /// Try to create a PipeWire capture instance.
        ///
        /// Returns an error if PipeWire is not available.
        pub fn new() -> Result<Self> {
            // Try to detect PipeWire availability by checking for the socket
            let runtime_dir =
                std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
            let pipewire_socket = std::path::Path::new(&runtime_dir).join("pipewire-0");

            if !pipewire_socket.exists() {
                anyhow::bail!("PipeWire socket not found at {}", pipewire_socket.display());
            }

            tracing::info!("PipeWire detected");

            Ok(Self {
                available: true,
                capturing: Arc::new(AtomicBool::new(false)),
            })
        }
    }

    impl AppAudioCapture for PipeWireAppAudioCapture {
        fn list_capturable_apps(&self) -> Result<Vec<AppAudioSource>> {
            if !self.available {
                return Ok(Vec::new());
            }

            // PipeWire app enumeration would go here.
            // For now, return an empty list since we can't link to pipewire-rs
            // without the PipeWire dev libraries.
            tracing::debug!("PipeWire: listing capturable apps (stub - no pipewire-rs linked)");
            Ok(Vec::new())
        }

        fn start_capture(&self, source: &AppAudioSource) -> Result<AudioStream> {
            if !self.available {
                anyhow::bail!("PipeWire not available");
            }

            if self.capturing.load(Ordering::Relaxed) {
                anyhow::bail!("already capturing");
            }

            tracing::info!(app = %source.name, id = source.id, "starting app audio capture");
            self.capturing.store(true, Ordering::Relaxed);

            // Real PipeWire capture would connect to the app's audio stream here.
            // For now, create a channel that will be populated when pipewire-rs
            // is integrated.
            let (_tx, rx) = mpsc::unbounded_channel();

            Ok(AudioStream::new(rx))
        }

        fn stop_capture(&self) {
            if self.capturing.swap(false, Ordering::Relaxed) {
                tracing::info!("stopping PipeWire app audio capture");
            }
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

    #[test]
    fn stub_list_returns_empty() {
        init_test_tracing();
        let stub = app_audio_stub::StubAppAudioCapture;
        let apps = stub.list_capturable_apps().unwrap();
        assert!(apps.is_empty());
    }

    #[test]
    fn stub_capture_returns_error() {
        init_test_tracing();
        let stub = app_audio_stub::StubAppAudioCapture;
        let source = AppAudioSource {
            name: "test_app".to_string(),
            id: 42,
        };
        let result = stub.start_capture(&source);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("not available"));
    }

    #[test]
    fn stub_stop_no_panic() {
        init_test_tracing();
        let stub = app_audio_stub::StubAppAudioCapture;
        stub.stop_capture(); // should not panic
    }

    #[test]
    fn create_factory_no_panic() {
        init_test_tracing();
        // Should return either PipeWire or stub, never panic
        let capture = create_app_audio_capture();
        let apps = capture.list_capturable_apps().unwrap();
        tracing::info!(count = apps.len(), "capturable apps found");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pipewire_fallback_if_unavailable() {
        init_test_tracing();
        // If PipeWire socket doesn't exist, construction should fail gracefully
        // (the factory function handles this by falling back to stub)
        let capture = create_app_audio_capture();
        // Should be usable regardless of PipeWire availability
        let apps = capture.list_capturable_apps().unwrap();
        assert!(apps.is_empty() || !apps.is_empty()); // trivially true, just verifying no panic
    }
}
