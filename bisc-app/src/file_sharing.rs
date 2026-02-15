//! File sharing integration: file picker, hashing, storage, and transfer.
//!
//! Connects the `FileStore`, `FileSender`/`FileReceiver`, and `SwarmDownloader`
//! from `bisc-files` with the Iced application layer.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use bisc_files::store::FileStore;
use bisc_files::swarm_download::{ChunkServer, SwarmDownloader, SwarmMetrics};
use bisc_files::transfer::CHUNK_SIZE;
use bisc_protocol::channel::ChannelMessage;
use bisc_protocol::file::FileManifest;
use bisc_protocol::types::EndpointId;
use sha2::{Digest, Sha256};

/// Manages file sharing state: file store and pending operations.
pub struct FileSharingState {
    /// The file store (SQLite-backed).
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

    /// Initialize the file store (creates database if needed).
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

    /// Get the file store (if initialized).
    pub fn store(&self) -> Option<&Arc<FileStore>> {
        self.store.as_ref()
    }

    /// Hash and register a file for sharing.
    ///
    /// Returns the file manifest and hash suitable for gossip announcement.
    pub async fn share_file(&self, file_path: &Path) -> Result<FileManifest> {
        let store = self.store.as_ref().context("file store not initialized")?;

        // Read the file
        let file_data = tokio::fs::read(file_path)
            .await
            .with_context(|| format!("failed to read file: {}", file_path.display()))?;

        let file_name = file_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let file_size = file_data.len() as u64;

        // Compute SHA-256 hash
        let mut hasher = Sha256::new();
        hasher.update(&file_data);
        let hash_result = hasher.finalize();
        let mut file_hash = [0u8; 32];
        file_hash.copy_from_slice(&hash_result);

        let chunk_count = file_size.div_ceil(CHUNK_SIZE as u64) as u32;

        let manifest = FileManifest {
            file_hash,
            file_name: file_name.clone(),
            file_size,
            chunk_size: CHUNK_SIZE,
            chunk_count,
        };

        // Register in store
        store.add_file(&manifest)?;

        // Copy file to storage directory
        let dest = store.file_path(&file_hash)?;
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::copy(file_path, &dest)
            .await
            .with_context(|| format!("failed to copy file to storage: {}", dest.display()))?;

        // Mark all chunks as received (we have the full file)
        for i in 0..chunk_count {
            store.set_chunk_received(&file_hash, i)?;
        }

        tracing::info!(
            file_name = %file_name,
            file_hash = data_encoding::HEXLOWER.encode(&file_hash),
            file_size = file_size,
            chunk_count = chunk_count,
            "file shared successfully"
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
        chunk_count: manifest.chunk_count,
    }
}

/// Spawn a task that accepts incoming bi-directional streams on the given
/// connection and serves file chunk requests via `ChunkServer`.
pub fn spawn_chunk_server(
    store: Arc<FileStore>,
    connection: iroh::endpoint::Connection,
    peer_id: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let metrics = Arc::new(SwarmMetrics::default());
        tracing::debug!(peer_id = %peer_id, "chunk server started");
        loop {
            match connection.accept_bi().await {
                Ok((mut send, mut recv)) => {
                    let s = store.clone();
                    let m = metrics.clone();
                    tokio::spawn(async move {
                        let server = ChunkServer::new(s, m);
                        if let Err(e) = server.handle_request(&mut send, &mut recv).await {
                            tracing::debug!(error = %e, "chunk server request failed");
                        }
                    });
                }
                Err(_) => {
                    tracing::debug!(peer_id = %peer_id, "chunk server: connection closed");
                    break;
                }
            }
        }
    })
}

/// Register a file manifest in the local store so it is ready for download.
pub fn register_announced_file(
    store: &FileStore,
    file_hash: &[u8; 32],
    file_name: &str,
    file_size: u64,
    chunk_count: u32,
) {
    let manifest = FileManifest {
        file_hash: *file_hash,
        file_name: file_name.to_string(),
        file_size,
        chunk_size: CHUNK_SIZE,
        chunk_count,
    };
    if let Err(e) = store.add_file(&manifest) {
        tracing::warn!(
            file_hash = data_encoding::HEXLOWER.encode(file_hash),
            error = %e,
            "failed to register announced file in store"
        );
    }
}

/// Download a file from peers using swarm download.
///
/// Opens bidirectional streams to each peer connection and downloads
/// missing chunks via the `SwarmDownloader`.
pub async fn download_file(
    store: Arc<FileStore>,
    file_hash: [u8; 32],
    connections: Vec<iroh::endpoint::Connection>,
) -> Result<()> {
    use bisc_files::swarm_download::PeerStream;

    let mut peer_streams = Vec::new();
    for (i, conn) in connections.iter().enumerate() {
        match conn.open_bi().await {
            Ok((send, recv)) => {
                peer_streams.push(PeerStream { send, recv, id: i });
            }
            Err(e) => {
                tracing::warn!(peer_idx = i, error = %e, "failed to open bi stream for download");
            }
        }
    }

    if peer_streams.is_empty() {
        anyhow::bail!("no peers available for download");
    }

    // Ensure the output file directory exists
    let file_path = store.file_path(&file_hash)?;
    if let Some(parent) = file_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    // Create empty file if it doesn't exist
    if !file_path.exists() {
        tokio::fs::write(&file_path, &[]).await?;
    }

    let downloader = SwarmDownloader::new();
    downloader
        .download(&file_hash, &mut peer_streams, &store)
        .await?;

    tracing::info!(
        file_hash = data_encoding::HEXLOWER.encode(&file_hash),
        "file download complete"
    );

    Ok(())
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
    async fn share_file_hashes_and_stores() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = FileSharingState::new(dir.path().to_path_buf());
        state.init().unwrap();

        // Create a test file
        let test_file = dir.path().join("hello.txt");
        tokio::fs::write(&test_file, b"Hello, world!")
            .await
            .unwrap();

        let manifest = state.share_file(&test_file).await.unwrap();
        assert_eq!(manifest.file_name, "hello.txt");
        assert_eq!(manifest.file_size, 13);
        assert_eq!(manifest.chunk_count, 1);

        // Verify it's in the store
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
    }

    #[test]
    fn file_announce_message_created() {
        let manifest = FileManifest {
            file_hash: [42u8; 32],
            file_name: "test.txt".to_string(),
            file_size: 100,
            chunk_size: CHUNK_SIZE,
            chunk_count: 1,
        };
        let endpoint_id = EndpointId([1u8; 32]);
        let msg = file_announce_message(&manifest, endpoint_id);

        match msg {
            ChannelMessage::FileAnnounce {
                file_name,
                file_size,
                chunk_count,
                ..
            } => {
                assert_eq!(file_name, "test.txt");
                assert_eq!(file_size, 100);
                assert_eq!(chunk_count, 1);
            }
            other => panic!("expected FileAnnounce, got {:?}", other),
        }
    }
}
