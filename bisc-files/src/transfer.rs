//! File chunking and transfer over reliable QUIC streams.
//!
//! Files are split into 256KB chunks, sent with length-prefixed postcard framing,
//! and reassembled on the receiver side with SHA-256 integrity verification.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use bisc_protocol::file::{FileChunk, FileManifest};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::store::FileStore;

/// Default chunk size: 256KB.
pub const CHUNK_SIZE: u32 = 262_144;

/// Transfer progress metrics.
#[derive(Debug, Default)]
pub struct TransferMetrics {
    pub bytes_transferred: AtomicU64,
    pub chunks_transferred: AtomicU64,
}

/// Sends files over a reliable bidirectional stream.
#[derive(Default)]
pub struct FileSender {
    metrics: Arc<TransferMetrics>,
}

impl FileSender {
    pub fn new() -> Self {
        Self {
            metrics: Arc::default(),
        }
    }

    /// Get transfer metrics.
    pub fn metrics(&self) -> &TransferMetrics {
        &self.metrics
    }

    /// Send a file over a bidirectional stream.
    ///
    /// 1. Reads the file, computes SHA-256 hash, builds the manifest.
    /// 2. Sends the manifest as a length-prefixed postcard message.
    /// 3. Sends each chunk as a length-prefixed `FileChunk` message.
    /// 4. Finishes the send stream.
    pub async fn send_file<S, R>(
        &self,
        path: &Path,
        send: &mut S,
        _recv: &mut R,
    ) -> Result<FileManifest>
    where
        S: AsyncWriteExt + Unpin,
        R: AsyncReadExt + Unpin,
    {
        let file_data = tokio::fs::read(path)
            .await
            .with_context(|| format!("failed to read file: {}", path.display()))?;

        let file_name = path
            .file_name()
            .context("path has no filename")?
            .to_string_lossy()
            .to_string();

        let file_size = file_data.len() as u64;
        let chunk_count = file_size.div_ceil(CHUNK_SIZE as u64) as u32;

        // Compute SHA-256
        let mut hasher = Sha256::new();
        hasher.update(&file_data);
        let hash_result = hasher.finalize();
        let mut file_hash = [0u8; 32];
        file_hash.copy_from_slice(&hash_result);

        let manifest = FileManifest {
            file_hash,
            file_name: file_name.clone(),
            file_size,
            chunk_size: CHUNK_SIZE,
            chunk_count,
        };

        tracing::info!(
            file_name = %file_name,
            file_size,
            chunk_count,
            file_hash = data_encoding::HEXLOWER.encode(&file_hash),
            "sending file"
        );

        // Send manifest first
        let manifest_bytes =
            postcard::to_allocvec(&manifest).context("failed to encode manifest")?;
        send.write_u32(manifest_bytes.len() as u32).await?;
        send.write_all(&manifest_bytes).await?;

        // Send chunks
        for i in 0..chunk_count {
            let start = i as usize * CHUNK_SIZE as usize;
            let end = std::cmp::min(start + CHUNK_SIZE as usize, file_data.len());
            let chunk_data = file_data[start..end].to_vec();
            let chunk_len = chunk_data.len() as u64;

            let chunk = FileChunk {
                file_hash,
                chunk_index: i,
                data: chunk_data,
            };

            let chunk_bytes = postcard::to_allocvec(&chunk).context("failed to encode chunk")?;
            send.write_u32(chunk_bytes.len() as u32).await?;
            send.write_all(&chunk_bytes).await?;

            self.metrics
                .bytes_transferred
                .fetch_add(chunk_len, Ordering::Relaxed);
            self.metrics
                .chunks_transferred
                .fetch_add(1, Ordering::Relaxed);

            tracing::debug!(chunk_index = i, chunk_size = chunk_len, "sent chunk");
        }

        send.flush().await?;

        tracing::info!(
            file_name = %manifest.file_name,
            chunks = chunk_count,
            "file send complete"
        );

        Ok(manifest)
    }
}

/// Receives files over a reliable bidirectional stream.
#[derive(Default)]
pub struct FileReceiver {
    metrics: Arc<TransferMetrics>,
}

impl FileReceiver {
    pub fn new() -> Self {
        Self {
            metrics: Arc::default(),
        }
    }

    /// Get transfer metrics.
    pub fn metrics(&self) -> &TransferMetrics {
        &self.metrics
    }

    /// Receive a file over a bidirectional stream and write it to the store.
    ///
    /// 1. Reads the manifest.
    /// 2. Registers the file in the store.
    /// 3. Reads each chunk, writes to disk, updates the store.
    /// 4. Verifies SHA-256 of the complete file.
    pub async fn receive_file<S, R>(
        &self,
        _send: &mut S,
        recv: &mut R,
        store: &FileStore,
    ) -> Result<FileManifest>
    where
        S: AsyncWriteExt + Unpin,
        R: AsyncReadExt + Unpin,
    {
        // Read manifest
        let manifest_len = recv
            .read_u32()
            .await
            .context("failed to read manifest length")?;
        let mut manifest_buf = vec![0u8; manifest_len as usize];
        recv.read_exact(&mut manifest_buf)
            .await
            .context("failed to read manifest")?;
        let manifest: FileManifest =
            postcard::from_bytes(&manifest_buf).context("failed to decode manifest")?;

        tracing::info!(
            file_name = %manifest.file_name,
            file_size = manifest.file_size,
            chunk_count = manifest.chunk_count,
            file_hash = data_encoding::HEXLOWER.encode(&manifest.file_hash),
            "receiving file"
        );

        // Register in store
        store.add_file(&manifest)?;
        let file_path = store.file_path(&manifest.file_hash)?;

        // Ensure parent directory exists
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Create/open the output file
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .await
            .with_context(|| format!("failed to create file: {}", file_path.display()))?;

        let mut writer = tokio::io::BufWriter::new(file);
        let mut hasher = Sha256::new();

        // Receive chunks
        for _ in 0..manifest.chunk_count {
            let chunk_len = recv
                .read_u32()
                .await
                .context("failed to read chunk length")?;
            let mut chunk_buf = vec![0u8; chunk_len as usize];
            recv.read_exact(&mut chunk_buf)
                .await
                .context("failed to read chunk data")?;

            let chunk: FileChunk =
                postcard::from_bytes(&chunk_buf).context("failed to decode chunk")?;

            if chunk.file_hash != manifest.file_hash {
                anyhow::bail!("chunk file_hash mismatch");
            }

            // Write chunk data at the correct offset
            let offset = chunk.chunk_index as u64 * manifest.chunk_size as u64;
            use tokio::io::AsyncSeekExt;
            writer.seek(std::io::SeekFrom::Start(offset)).await?;
            writer.write_all(&chunk.data).await?;

            hasher.update(&chunk.data);

            store.set_chunk_received(&manifest.file_hash, chunk.chunk_index)?;

            self.metrics
                .bytes_transferred
                .fetch_add(chunk.data.len() as u64, Ordering::Relaxed);
            self.metrics
                .chunks_transferred
                .fetch_add(1, Ordering::Relaxed);

            tracing::debug!(
                chunk_index = chunk.chunk_index,
                chunk_size = chunk.data.len(),
                "received chunk"
            );
        }

        writer.flush().await?;

        // Verify SHA-256
        let computed_hash = hasher.finalize();
        if computed_hash.as_slice() != manifest.file_hash {
            anyhow::bail!(
                "SHA-256 mismatch: expected {}, got {}",
                data_encoding::HEXLOWER.encode(&manifest.file_hash),
                data_encoding::HEXLOWER.encode(computed_hash.as_slice()),
            );
        }

        tracing::info!(
            file_name = %manifest.file_name,
            file_hash = data_encoding::HEXLOWER.encode(&manifest.file_hash),
            "file receive complete, SHA-256 verified"
        );

        Ok(manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn init_test_tracing() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "debug".into()),
            )
            .with_test_writer()
            .try_init();
    }

    /// Create a test file with deterministic content.
    async fn create_test_file(
        dir: &Path,
        name: &str,
        size: usize,
    ) -> (std::path::PathBuf, [u8; 32]) {
        let path = dir.join(name);
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let mut hasher = Sha256::new();
        hasher.update(&data);
        let hash_result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hash_result);
        tokio::fs::write(&path, &data).await.unwrap();
        (path, hash)
    }

    #[tokio::test]
    async fn send_receive_small_file() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let store_dir = TempDir::new().unwrap();

        let (file_path, expected_hash) = create_test_file(tmp.path(), "small.bin", 1000).await;

        let store = FileStore::new(store_dir.path().to_path_buf()).unwrap();

        // Use a duplex stream to simulate a connection
        let (client, server) = tokio::io::duplex(1024 * 1024);
        let (mut client_read, mut client_write) = tokio::io::split(client);
        let (mut server_read, mut server_write) = tokio::io::split(server);

        let sender = FileSender::new();
        let receiver = FileReceiver::new();

        let send_handle = tokio::spawn(async move {
            sender
                .send_file(&file_path, &mut client_write, &mut client_read)
                .await
                .unwrap()
        });

        let recv_handle = tokio::spawn(async move {
            receiver
                .receive_file(&mut server_write, &mut server_read, &store)
                .await
                .unwrap()
        });

        let sent_manifest = send_handle.await.unwrap();
        let recv_manifest = recv_handle.await.unwrap();

        assert_eq!(sent_manifest, recv_manifest);
        assert_eq!(sent_manifest.file_hash, expected_hash);
        assert_eq!(sent_manifest.chunk_count, 1);
        assert_eq!(sent_manifest.file_size, 1000);
    }

    #[tokio::test]
    async fn send_receive_large_file() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let store_dir = TempDir::new().unwrap();

        // 1.5 MB = 6 chunks of 256KB + partial
        let size = 1_500_000;
        let (file_path, expected_hash) = create_test_file(tmp.path(), "large.bin", size).await;

        let store = FileStore::new(store_dir.path().to_path_buf()).unwrap();

        let (client, server) = tokio::io::duplex(1024 * 1024);
        let (mut client_read, mut client_write) = tokio::io::split(client);
        let (mut server_read, mut server_write) = tokio::io::split(server);

        let sender = FileSender::new();
        let receiver = FileReceiver::new();

        let send_handle = tokio::spawn(async move {
            sender
                .send_file(&file_path, &mut client_write, &mut client_read)
                .await
                .unwrap()
        });

        let store_clone = FileStore::new(store_dir.path().to_path_buf()).unwrap();
        let recv_handle = tokio::spawn(async move {
            receiver
                .receive_file(&mut server_write, &mut server_read, &store)
                .await
                .unwrap()
        });

        let sent_manifest = send_handle.await.unwrap();
        let recv_manifest = recv_handle.await.unwrap();

        assert_eq!(sent_manifest, recv_manifest);
        assert_eq!(sent_manifest.file_hash, expected_hash);
        assert_eq!(sent_manifest.file_size, size as u64);
        // 1_500_000 / 262_144 = 5.72 => 6 chunks
        assert_eq!(sent_manifest.chunk_count, 6);

        // Verify the store shows the file as complete
        assert!(store_clone.is_complete(&expected_hash).unwrap());

        // Verify the received file matches
        let received_path = store_clone.file_path(&expected_hash).unwrap();
        let received_data = tokio::fs::read(&received_path).await.unwrap();
        assert_eq!(received_data.len(), size);

        // Verify content
        let original_data = tokio::fs::read(tmp.path().join("large.bin")).await.unwrap();
        assert_eq!(received_data, original_data);
    }

    #[tokio::test]
    async fn connection_drop_returns_error() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let store_dir = TempDir::new().unwrap();

        let (file_path, _) = create_test_file(tmp.path(), "drop.bin", 500_000).await;

        let _store = FileStore::new(store_dir.path().to_path_buf()).unwrap();

        // Small buffer to cause backpressure, then drop the receiver early
        let (client, server) = tokio::io::duplex(4096);
        let (mut client_read, mut client_write) = tokio::io::split(client);
        let (mut server_read, server_write) = tokio::io::split(server);

        let sender = FileSender::new();

        let send_handle = tokio::spawn(async move {
            sender
                .send_file(&file_path, &mut client_write, &mut client_read)
                .await
        });

        // Read only the manifest, then drop the connection
        let manifest_len = server_read.read_u32().await.unwrap();
        let mut buf = vec![0u8; manifest_len as usize];
        server_read.read_exact(&mut buf).await.unwrap();
        drop(server_read);
        drop(server_write);

        // Sender should get an error, not panic
        let result = send_handle.await.unwrap();
        assert!(result.is_err(), "send should fail when connection drops");
        tracing::info!("connection drop error: {}", result.unwrap_err());
    }

    #[tokio::test]
    async fn metrics_track_transfer() {
        init_test_tracing();
        let tmp = TempDir::new().unwrap();
        let store_dir = TempDir::new().unwrap();

        let (file_path, _) =
            create_test_file(tmp.path(), "metrics.bin", CHUNK_SIZE as usize * 3).await;

        let store = FileStore::new(store_dir.path().to_path_buf()).unwrap();

        let (client, server) = tokio::io::duplex(1024 * 1024);
        let (mut client_read, mut client_write) = tokio::io::split(client);
        let (mut server_read, mut server_write) = tokio::io::split(server);

        let sender = FileSender::new();
        let receiver = FileReceiver::new();
        let sender_metrics = Arc::clone(&sender.metrics);
        let receiver_metrics = Arc::clone(&receiver.metrics);

        let send_handle = tokio::spawn(async move {
            sender
                .send_file(&file_path, &mut client_write, &mut client_read)
                .await
                .unwrap()
        });

        let recv_handle = tokio::spawn(async move {
            receiver
                .receive_file(&mut server_write, &mut server_read, &store)
                .await
                .unwrap()
        });

        send_handle.await.unwrap();
        recv_handle.await.unwrap();

        assert_eq!(sender_metrics.chunks_transferred.load(Ordering::Relaxed), 3);
        assert_eq!(
            receiver_metrics.chunks_transferred.load(Ordering::Relaxed),
            3
        );
        assert_eq!(
            sender_metrics.bytes_transferred.load(Ordering::Relaxed),
            CHUNK_SIZE as u64 * 3
        );
    }
}
