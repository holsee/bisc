//! BiscTicket creation, parsing, and topic derivation.

use data_encoding::BASE32_NOPAD;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::types::{EndpointAddr, TopicId};

/// A ticket that encodes everything needed to join a channel.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BiscTicket {
    pub channel_secret: [u8; 32],
    pub bootstrap_addrs: Vec<EndpointAddr>,
}

/// Errors when parsing a `BiscTicket` from a string.
#[derive(Debug, Error)]
pub enum TicketError {
    #[error("missing 'bisc1' prefix")]
    MissingPrefix,
    #[error("invalid base32 encoding: {0}")]
    Base32Decode(#[from] data_encoding::DecodeError),
    #[error("invalid ticket data: {0}")]
    Deserialize(#[from] postcard::Error),
}

/// Derive a `TopicId` deterministically from a channel secret.
pub fn topic_from_secret(secret: &[u8; 32]) -> TopicId {
    let hash = Sha256::new()
        .chain_update(b"bisc-channel-v1:")
        .chain_update(secret)
        .finalize();
    let mut topic = [0u8; 32];
    topic.copy_from_slice(&hash);
    TopicId(topic)
}

impl BiscTicket {
    /// Encode the ticket as a `bisc1`-prefixed base32 string.
    pub fn to_ticket_string(&self) -> String {
        let bytes = postcard::to_allocvec(self).expect("ticket serialization should not fail");
        let encoded = BASE32_NOPAD.encode(&bytes);
        format!("bisc1{}", encoded.to_lowercase())
    }

    /// Parse a ticket from a `bisc1`-prefixed base32 string.
    pub fn from_ticket_str(s: &str) -> Result<Self, TicketError> {
        let s = s.trim();
        if !s.starts_with("bisc1") {
            return Err(TicketError::MissingPrefix);
        }
        let b32 = &s[5..];
        let bytes = BASE32_NOPAD.decode(b32.to_uppercase().as_bytes())?;
        let ticket: BiscTicket = postcard::from_bytes(&bytes)?;
        Ok(ticket)
    }
}

impl std::fmt::Display for BiscTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_ticket_string())
    }
}

impl std::str::FromStr for BiscTicket {
    type Err = TicketError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_ticket_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EndpointId;

    #[test]
    fn ticket_roundtrip() {
        let ticket = BiscTicket {
            channel_secret: [0x42; 32],
            bootstrap_addrs: vec![EndpointAddr {
                id: EndpointId([0x01; 32]),
                addrs: vec!["192.168.1.1:12345".to_string()],
                relay_url: Some("https://relay.example.com".to_string()),
            }],
        };
        let s = ticket.to_ticket_string();
        assert!(s.starts_with("bisc1"));

        let parsed: BiscTicket = s.parse().unwrap();
        assert_eq!(ticket, parsed);
    }

    #[test]
    fn ticket_roundtrip_no_relay() {
        let ticket = BiscTicket {
            channel_secret: [0xFF; 32],
            bootstrap_addrs: vec![EndpointAddr {
                id: EndpointId([0xAA; 32]),
                addrs: vec!["10.0.0.1:9000".to_string(), "10.0.0.2:9001".to_string()],
                relay_url: None,
            }],
        };
        let s = ticket.to_ticket_string();
        let parsed: BiscTicket = s.parse().unwrap();
        assert_eq!(ticket, parsed);
    }

    #[test]
    fn ticket_roundtrip_multiple_bootstrap() {
        let ticket = BiscTicket {
            channel_secret: [0x00; 32],
            bootstrap_addrs: vec![
                EndpointAddr {
                    id: EndpointId([0x01; 32]),
                    addrs: vec!["1.2.3.4:5000".to_string()],
                    relay_url: None,
                },
                EndpointAddr {
                    id: EndpointId([0x02; 32]),
                    addrs: vec!["5.6.7.8:6000".to_string()],
                    relay_url: Some("https://relay2.example.com".to_string()),
                },
            ],
        };
        let s = ticket.to_ticket_string();
        let parsed: BiscTicket = s.parse().unwrap();
        assert_eq!(ticket, parsed);
    }

    #[test]
    fn ticket_missing_prefix() {
        let result = BiscTicket::from_ticket_str("invalid_ticket_string");
        assert!(result.is_err());
        assert!(matches!(result, Err(TicketError::MissingPrefix)));
    }

    #[test]
    fn ticket_display_and_from_str_agree() {
        let ticket = BiscTicket {
            channel_secret: [0x55; 32],
            bootstrap_addrs: vec![EndpointAddr {
                id: EndpointId([0x77; 32]),
                addrs: vec!["127.0.0.1:8080".to_string()],
                relay_url: None,
            }],
        };
        let display = format!("{ticket}");
        let parsed: BiscTicket = display.parse().unwrap();
        assert_eq!(ticket, parsed);
    }

    #[test]
    fn topic_from_secret_is_deterministic() {
        let secret = [0xAB; 32];
        let t1 = topic_from_secret(&secret);
        let t2 = topic_from_secret(&secret);
        assert_eq!(t1, t2);
    }

    #[test]
    fn topic_from_secret_different_secrets_produce_different_topics() {
        let t1 = topic_from_secret(&[0x00; 32]);
        let t2 = topic_from_secret(&[0x01; 32]);
        assert_ne!(t1, t2);
    }
}
