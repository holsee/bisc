//! File sharing integration using iroh-blobs for content-addressed transfers.
//!
//! Files are added to an iroh-blobs `MemStore`, announced via gossip, and
//! downloaded by peers via the iroh-blobs protocol.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use bisc_files::store::FileStore;
use bisc_protocol::channel::ChannelMessage;
use bisc_protocol::file::FileManifest;
use bisc_protocol::types::EndpointId;
use iroh_blobs::store::mem::MemStore;

/// Manages file sharing state: metadata store and blob store.
pub struct FileSharingState {
    /// The file metadata store (SQLite-backed).
    store: Option<Arc<FileStore>>,
    /// Storage directory path.
    storage_dir: PathBuf,
}

impl FileSharingState {
    /// Create a new file sharing state with the given storage directory.
    pub fn new(storage_dir: PathBuf) -> Self {
        Self {
            store: None,
            storage_dir,
        }
    }

    /// Initialize the metadata store (creates database if needed).
    pub fn init(&mut self) -> Result<()> {
        if self.store.is_some() {
            return Ok(());
        }

        let store =
            FileStore::new(self.storage_dir.clone()).context("failed to initialize file store")?;
        self.store = Some(Arc::new(store));
        tracing::info!(storage_dir = %self.storage_dir.display(), "file sharing initialized");
        Ok(())
    }

    /// Get the metadata store (if initialized).
    pub fn store(&self) -> Option<&Arc<FileStore>> {
        self.store.as_ref()
    }

    /// Add a file to the blob store and register metadata.
    ///
    /// Returns the file manifest with BLAKE3 hash.
    pub async fn share_file(
        &self,
        file_path: &Path,
        blob_store: &MemStore,
    ) -> Result<FileManifest> {
        let store = self.store.as_ref().context("file store not initialized")?;

        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let file_size = tokio::fs::metadata(file_path)
            .await
            .with_context(|| format!("failed to stat file: {}", file_path.display()))?
            .len();

        // Add file to iroh-blobs store
        let tag_info = blob_store
            .blobs()
            .add_path(file_path)
            .await
            .with_context(|| {
                format!("failed to add file to blob store: {}", file_path.display())
            })?;

        let hash_bytes: [u8; 32] = *tag_info.hash.as_bytes();

        let manifest = FileManifest {
            file_hash: hash_bytes,
            file_name: file_name.clone(),
            file_size,
        };

        // Register in metadata store
        store.add_file(&manifest)?;
        store.set_complete(&hash_bytes)?;

        tracing::info!(
            file_name = %file_name,
            file_hash = data_encoding::HEXLOWER.encode(&hash_bytes),
            file_size = file_size,
            "file shared via iroh-blobs"
        );

        Ok(manifest)
    }
}

/// Open a native file picker dialog.
///
/// Returns the selected file path, or None if cancelled.
pub async fn pick_file() -> Option<PathBuf> {
    let handle = rfd::AsyncFileDialog::new()
        .set_title("Share a file")
        .pick_file()
        .await?;
    Some(handle.path().to_path_buf())
}

/// Create a `ChannelMessage::FileAnnounce` from a manifest and endpoint ID.
pub fn file_announce_message(manifest: &FileManifest, endpoint_id: EndpointId) -> ChannelMessage {
    ChannelMessage::FileAnnounce {
        endpoint_id,
        file_hash: manifest.file_hash,
        file_name: manifest.file_name.clone(),
        file_size: manifest.file_size,
    }
}

/// Register a file manifest in the local metadata store so it appears in the UI.
pub fn register_announced_file(
    store: &FileStore,
    file_hash: &[u8; 32],
    file_name: &str,
    file_size: u64,
) {
    let manifest = FileManifest {
        file_hash: *file_hash,
        file_name: file_name.to_string(),
        file_size,
    };
    if let Err(e) = store.add_file(&manifest) {
        tracing::warn!(
            file_hash = data_encoding::HEXLOWER.encode(file_hash),
            error = %e,
            "failed to register announced file in store"
        );
    }
}

/// Download a file from a peer using iroh-blobs.
///
/// Connects to the peer on the iroh-blobs ALPN and downloads the blob.
pub async fn download_file(
    blob_store: &MemStore,
    file_hash: [u8; 32],
    endpoint: &iroh::Endpoint,
    peer_node_ids: Vec<iroh::EndpointId>,
) -> Result<()> {
    let hash = iroh_blobs::Hash::from_bytes(file_hash);

    for node_id in &peer_node_ids {
        tracing::debug!(
            peer = %node_id,
            hash = %hash.to_hex(),
            "attempting blob download from peer"
        );

        match endpoint.connect(*node_id, iroh_blobs::ALPN).await {
            Ok(connection) => {
                let request = iroh_blobs::protocol::GetRequest::blob(hash);
                let progress = blob_store.remote().execute_get(connection, request);
                match progress.await {
                    Ok(_stats) => {
                        tracing::info!(
                            hash = %hash.to_hex(),
                            "blob download complete"
                        );
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!(
                            peer = %node_id,
                            error = %e,
                            "blob download failed from peer, trying next"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    peer = %node_id,
                    error = %e,
                    "failed to connect to peer for blob download"
                );
            }
        }
    }

    anyhow::bail!("no peers available for download")
}

/// Export a downloaded blob to a filesystem path.
#[allow(dead_code)]
pub async fn export_blob(
    blob_store: &MemStore,
    file_hash: [u8; 32],
    destination: &Path,
) -> Result<u64> {
    let hash = iroh_blobs::Hash::from_bytes(file_hash);

    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let size = blob_store
        .blobs()
        .export(hash, destination)
        .await
        .with_context(|| format!("failed to export blob to {}", destination.display()))?;

    tracing::info!(
        hash = %hash.to_hex(),
        size,
        destination = %destination.display(),
        "blob exported to filesystem"
    );

    Ok(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_sharing_state_starts_uninitialised() {
        let state = FileSharingState::new(PathBuf::from("/tmp/test-bisc-files"));
        assert!(state.store.is_none());
    }

    #[test]
    fn init_creates_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = FileSharingState::new(dir.path().to_path_buf());
        state.init().unwrap();
        assert!(state.store.is_some());
    }

    #[test]
    fn init_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = FileSharingState::new(dir.path().to_path_buf());
        state.init().unwrap();
        state.init().unwrap();
        assert!(state.store.is_some());
    }

    #[tokio::test]
    async fn share_file_adds_to_blob_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = FileSharingState::new(dir.path().to_path_buf());
        state.init().unwrap();

        let blob_store = MemStore::new();

        // Create a test file
        let test_file = dir.path().join("hello.txt");
        tokio::fs::write(&test_file, b"Hello, world!")
            .await
            .unwrap();

        let manifest = state.share_file(&test_file, &blob_store).await.unwrap();
        assert_eq!(manifest.file_name, "hello.txt");
        assert_eq!(manifest.file_size, 13);

        // Verify it's in the metadata store
        let stored = state
            .store
            .as_ref()
            .unwrap()
            .get_file(&manifest.file_hash)
            .unwrap();
        assert!(stored.is_some());
        assert!(state
            .store
            .as_ref()
            .unwrap()
            .is_complete(&manifest.file_hash)
            .unwrap());

        // Verify it's in the blob store
        let hash = iroh_blobs::Hash::from_bytes(manifest.file_hash);
        let has_blob = blob_store.blobs().has(hash).await.unwrap();
        assert!(has_blob);
    }

    #[test]
    fn file_announce_message_created() {
        let manifest = FileManifest {
            file_hash: [42u8; 32],
            file_name: "test.txt".to_string(),
            file_size: 100,
        };
        let endpoint_id = EndpointId([1u8; 32]);
        let msg = file_announce_message(&manifest, endpoint_id);

        match msg {
            ChannelMessage::FileAnnounce {
                file_name,
                file_size,
                ..
            } => {
                assert_eq!(file_name, "test.txt");
                assert_eq!(file_size, 100);
            }
            other => panic!("expected FileAnnounce, got {:?}", other),
        }
    }
}
