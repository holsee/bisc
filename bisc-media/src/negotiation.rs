//! Media negotiation: offer/answer exchange over a reliable QUIC stream.
//!
//! When two peers connect, they negotiate codecs and quality parameters.
//! The offerer sends a `MediaOffer` with local capabilities, and the
//! answerer selects the best matching codecs and quality, responding
//! with a `MediaAnswer`.

use anyhow::{Context, Result};
use bisc_protocol::media::{CodecInfo, MediaAnswer, MediaOffer};
use iroh::endpoint::{RecvStream, SendStream};

/// Write a length-prefixed postcard-encoded message to a stream.
async fn write_message<T: serde::Serialize>(stream: &mut SendStream, msg: &T) -> Result<()> {
    let data = postcard::to_allocvec(msg).context("failed to serialize message")?;
    let len = (data.len() as u32).to_be_bytes();
    stream
        .write_all(&len)
        .await
        .context("failed to write message length")?;
    stream
        .write_all(&data)
        .await
        .context("failed to write message data")?;
    Ok(())
}

/// Read a length-prefixed postcard-encoded message from a stream.
async fn read_message<T: serde::de::DeserializeOwned>(stream: &mut RecvStream) -> Result<T> {
    let mut len_buf = [0u8; 4];
    stream
        .read_exact(&mut len_buf)
        .await
        .context("failed to read message length")?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 64 * 1024 {
        anyhow::bail!("message too large: {len} bytes");
    }

    let mut data = vec![0u8; len];
    stream
        .read_exact(&mut data)
        .await
        .context("failed to read message data")?;

    postcard::from_bytes(&data).context("failed to deserialize message")
}

/// Default local capabilities for offer generation.
pub fn default_offer() -> MediaOffer {
    MediaOffer {
        audio_codecs: vec![CodecInfo {
            name: "opus".to_string(),
            sample_rate: 48_000,
            channels: 1,
        }],
        video_codecs: vec![
            CodecInfo {
                name: "h264".to_string(),
                sample_rate: 90_000,
                channels: 0,
            },
            CodecInfo {
                name: "vp8".to_string(),
                sample_rate: 90_000,
                channels: 0,
            },
        ],
        max_resolution: (1920, 1080),
        max_framerate: 30,
    }
}

/// Negotiate as the offerer: send our capabilities, receive the answer.
pub async fn negotiate_as_offerer(
    send: &mut SendStream,
    recv: &mut RecvStream,
    offer: &MediaOffer,
) -> Result<MediaAnswer> {
    tracing::info!("sending media offer");
    write_message(send, offer).await?;

    tracing::debug!("waiting for media answer");
    let answer: MediaAnswer = read_message(recv).await?;

    tracing::info!(
        audio_codec = %answer.selected_audio_codec.name,
        video_codec = %answer.selected_video_codec.name,
        resolution = ?answer.negotiated_resolution,
        framerate = answer.negotiated_framerate,
        "media negotiation complete (offerer)"
    );

    Ok(answer)
}

/// Negotiate as the answerer: receive the offer, select best parameters, send answer.
pub async fn negotiate_as_answerer(
    send: &mut SendStream,
    recv: &mut RecvStream,
    local_offer: &MediaOffer,
) -> Result<MediaAnswer> {
    tracing::debug!("waiting for media offer");
    let remote_offer: MediaOffer = read_message(recv).await?;

    tracing::info!("received media offer, selecting codecs");
    let answer = select_answer(&remote_offer, local_offer)?;

    tracing::info!(
        audio_codec = %answer.selected_audio_codec.name,
        video_codec = %answer.selected_video_codec.name,
        resolution = ?answer.negotiated_resolution,
        framerate = answer.negotiated_framerate,
        "sending media answer"
    );

    write_message(send, &answer).await?;

    Ok(answer)
}

/// Select the best matching codecs and quality from two offers.
pub fn select_answer(remote: &MediaOffer, local: &MediaOffer) -> Result<MediaAnswer> {
    let audio_codec = select_codec(&remote.audio_codecs, &local.audio_codecs, &["opus"])
        .context("no common audio codec")?;

    let video_codec = select_codec(&remote.video_codecs, &local.video_codecs, &["h264", "vp8"])
        .context("no common video codec")?;

    let negotiated_resolution = (
        remote.max_resolution.0.min(local.max_resolution.0),
        remote.max_resolution.1.min(local.max_resolution.1),
    );

    let negotiated_framerate = remote.max_framerate.min(local.max_framerate);

    Ok(MediaAnswer {
        selected_audio_codec: audio_codec,
        selected_video_codec: video_codec,
        negotiated_resolution,
        negotiated_framerate,
    })
}

/// Select the best matching codec from two lists, with priority preference.
fn select_codec(remote: &[CodecInfo], local: &[CodecInfo], priority: &[&str]) -> Option<CodecInfo> {
    // Find common codecs
    let mut common: Vec<&CodecInfo> = remote
        .iter()
        .filter(|r| local.iter().any(|l| l.name == r.name))
        .collect();

    if common.is_empty() {
        return None;
    }

    // Sort by priority preference
    common.sort_by_key(|c| {
        priority
            .iter()
            .position(|&p| p == c.name)
            .unwrap_or(usize::MAX)
    });

    Some(common[0].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_answer_picks_lower_resolution() {
        let remote = MediaOffer {
            audio_codecs: vec![CodecInfo {
                name: "opus".to_string(),
                sample_rate: 48_000,
                channels: 1,
            }],
            video_codecs: vec![CodecInfo {
                name: "h264".to_string(),
                sample_rate: 90_000,
                channels: 0,
            }],
            max_resolution: (1920, 1080),
            max_framerate: 30,
        };

        let local = MediaOffer {
            audio_codecs: vec![CodecInfo {
                name: "opus".to_string(),
                sample_rate: 48_000,
                channels: 1,
            }],
            video_codecs: vec![CodecInfo {
                name: "h264".to_string(),
                sample_rate: 90_000,
                channels: 0,
            }],
            max_resolution: (1280, 720),
            max_framerate: 24,
        };

        let answer = select_answer(&remote, &local).unwrap();
        assert_eq!(answer.negotiated_resolution, (1280, 720));
        assert_eq!(answer.negotiated_framerate, 24);
    }

    #[test]
    fn select_answer_no_common_codec_returns_error() {
        let remote = MediaOffer {
            audio_codecs: vec![CodecInfo {
                name: "aac".to_string(),
                sample_rate: 44_100,
                channels: 2,
            }],
            video_codecs: vec![CodecInfo {
                name: "h264".to_string(),
                sample_rate: 90_000,
                channels: 0,
            }],
            max_resolution: (1920, 1080),
            max_framerate: 30,
        };

        let local = MediaOffer {
            audio_codecs: vec![CodecInfo {
                name: "opus".to_string(),
                sample_rate: 48_000,
                channels: 1,
            }],
            video_codecs: vec![CodecInfo {
                name: "h264".to_string(),
                sample_rate: 90_000,
                channels: 0,
            }],
            max_resolution: (1920, 1080),
            max_framerate: 30,
        };

        let result = select_answer(&remote, &local);
        assert!(result.is_err(), "should fail when no common audio codec");
    }

    #[test]
    fn codec_priority_prefers_h264_over_vp8() {
        let remote = MediaOffer {
            audio_codecs: vec![CodecInfo {
                name: "opus".to_string(),
                sample_rate: 48_000,
                channels: 1,
            }],
            video_codecs: vec![
                CodecInfo {
                    name: "vp8".to_string(),
                    sample_rate: 90_000,
                    channels: 0,
                },
                CodecInfo {
                    name: "h264".to_string(),
                    sample_rate: 90_000,
                    channels: 0,
                },
            ],
            max_resolution: (1920, 1080),
            max_framerate: 30,
        };

        let local = MediaOffer {
            audio_codecs: vec![CodecInfo {
                name: "opus".to_string(),
                sample_rate: 48_000,
                channels: 1,
            }],
            video_codecs: vec![
                CodecInfo {
                    name: "h264".to_string(),
                    sample_rate: 90_000,
                    channels: 0,
                },
                CodecInfo {
                    name: "vp8".to_string(),
                    sample_rate: 90_000,
                    channels: 0,
                },
            ],
            max_resolution: (1920, 1080),
            max_framerate: 30,
        };

        let answer = select_answer(&remote, &local).unwrap();
        assert_eq!(answer.selected_video_codec.name, "h264");
    }

    #[test]
    fn length_prefix_encoding_roundtrip() {
        // Test the length-prefix framing by encoding/decoding manually
        let offer = default_offer();
        let data = postcard::to_allocvec(&offer).unwrap();
        let len_bytes = (data.len() as u32).to_be_bytes();

        // Reconstruct
        let decoded_len = u32::from_be_bytes(len_bytes) as usize;
        assert_eq!(decoded_len, data.len());

        let decoded: MediaOffer = postcard::from_bytes(&data).unwrap();
        assert_eq!(decoded.audio_codecs[0].name, "opus");
        assert_eq!(decoded.max_framerate, 30);
    }
}
