//! BiscEndpoint wrapper around `iroh::Endpoint`.

use anyhow::Result;
use iroh::{Endpoint, EndpointId, RelayMode};

/// ALPN protocol identifier for bisc media connections.
pub const MEDIA_ALPN: &[u8] = b"bisc/media/0";

/// Wrapper around an iroh `Endpoint` configured for bisc.
#[derive(Debug, Clone)]
pub struct BiscEndpoint {
    endpoint: Endpoint,
}

impl BiscEndpoint {
    /// Create a new endpoint with bisc ALPN protocols and default relay configuration.
    pub async fn new() -> Result<Self> {
        let endpoint = Endpoint::builder()
            .alpns(vec![MEDIA_ALPN.to_vec(), iroh_blobs::ALPN.to_vec()])
            .bind()
            .await?;

        tracing::info!(endpoint_id = %endpoint.id(), "bisc endpoint created");

        Ok(Self { endpoint })
    }

    /// Create an endpoint for testing: no relay servers, no DNS discovery.
    ///
    /// Uses `RelayMode::Disabled` so the endpoint only connects via direct
    /// localhost addresses — zero external network dependency.
    pub async fn for_testing() -> Result<Self> {
        let endpoint = Endpoint::empty_builder(RelayMode::Disabled)
            .alpns(vec![MEDIA_ALPN.to_vec(), iroh_blobs::ALPN.to_vec()])
            .bind()
            .await?;

        tracing::info!(endpoint_id = %endpoint.id(), "bisc test endpoint created (relay disabled)");

        Ok(Self { endpoint })
    }

    /// Get the underlying iroh endpoint.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Get the endpoint's ID (public key).
    pub fn id(&self) -> EndpointId {
        self.endpoint.id()
    }

    /// Shut down the endpoint gracefully.
    pub async fn close(&self) {
        tracing::info!("shutting down bisc endpoint");
        self.endpoint.close().await;
    }
}
