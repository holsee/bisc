//! Integration tests for audio I/O.
//!
//! These tests require the `audio` feature and a system with audio support.
//! On systems without audio hardware, tests skip gracefully.

#![cfg(feature = "audio")]

use std::time::Duration;

use bisc_media::audio_io::{AudioConfig, AudioInput, AudioOutput};

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
fn list_input_devices_returns_list_or_empty() {
    init_test_tracing();

    match bisc_media::audio_io::list_input_devices() {
        Ok(devices) => {
            tracing::info!(count = devices.len(), "input devices enumerated");
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not enumerate input devices (no audio support?)");
        }
    }
}

#[test]
fn list_output_devices_returns_list_or_empty() {
    init_test_tracing();

    match bisc_media::audio_io::list_output_devices() {
        Ok(devices) => {
            tracing::info!(count = devices.len(), "output devices enumerated");
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not enumerate output devices (no audio support?)");
        }
    }
}

#[test]
fn audio_input_create_start_stop() {
    init_test_tracing();

    let config = AudioConfig::default();
    let result = AudioInput::new(&config);

    match result {
        Ok((input, _rx)) => {
            assert!(!input.is_running());

            if let Err(e) = input.start() {
                tracing::warn!(error = %e, "could not start audio input (no device?)");
                return;
            }
            assert!(input.is_running());

            std::thread::sleep(Duration::from_millis(100));

            input.stop().expect("stop should not fail");
            assert!(!input.is_running());

            tracing::info!("audio input create/start/stop succeeded");
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not create audio input (no device?)");
        }
    }
}

#[test]
fn audio_output_create_start_stop() {
    init_test_tracing();

    let config = AudioConfig::default();
    let result = AudioOutput::new(&config);

    match result {
        Ok(output) => {
            assert!(!output.is_running());

            if let Err(e) = output.start() {
                tracing::warn!(error = %e, "could not start audio output (no device?)");
                return;
            }
            assert!(output.is_running());

            std::thread::sleep(Duration::from_millis(100));

            output.stop().expect("stop should not fail");
            assert!(!output.is_running());

            tracing::info!("audio output create/start/stop succeeded");
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not create audio output (no device?)");
        }
    }
}

#[test]
fn audio_loopback_pipeline_runs_without_errors() {
    init_test_tracing();

    let config = AudioConfig::default();

    let input_result = AudioInput::new(&config);
    let output_result = AudioOutput::new(&config);

    let (input, mut rx) = match input_result {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "skipping loopback test: no input device");
            return;
        }
    };

    let output = match output_result {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "skipping loopback test: no output device");
            return;
        }
    };

    let sender = output.sender();

    // Start both
    if let Err(e) = input.start() {
        tracing::warn!(error = %e, "skipping loopback test: could not start input");
        return;
    }
    if let Err(e) = output.start() {
        tracing::warn!(error = %e, "skipping loopback test: could not start output");
        return;
    }

    // Run for 1 second, forwarding captured audio to output
    let start = std::time::Instant::now();
    let mut frames_forwarded = 0u64;

    while start.elapsed() < Duration::from_secs(1) {
        match rx.try_recv() {
            Ok(frame) => {
                let _ = sender.send(frame);
                frames_forwarded += 1;
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    input.stop().expect("stop input");
    output.stop().expect("stop output");

    tracing::info!(frames = frames_forwarded, "loopback test completed");
}
