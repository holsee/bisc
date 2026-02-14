//! Peer-assisted file downloads: download chunks from any peer that has them.
//!
//! - `ChunkServer` handles incoming chunk requests.
//! - `SwarmDownloader` distributes chunk downloads across available peers.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use bisc_protocol::file::{
    decode_file_chunk, decode_file_request, encode_file_chunk, encode_file_request, ChunkBitfield,
    FileChunk, FileRequest,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::store::FileStore;
use crate::transfer::CHUNK_SIZE;

/// Metrics for swarm downloads.
#[derive(Debug, Default)]
pub struct SwarmMetrics {
    pub chunks_downloaded: AtomicU64,
    pub chunks_served: AtomicU64,
    pub bytes_downloaded: AtomicU64,
    pub bytes_served: AtomicU64,
    pub peer_failures: AtomicU64,
}

/// Serves file chunks to requesting peers.
pub struct ChunkServer {
    store: Arc<FileStore>,
    metrics: Arc<SwarmMetrics>,
}

impl ChunkServer {
    pub fn new(store: Arc<FileStore>, metrics: Arc<SwarmMetrics>) -> Self {
        Self { store, metrics }
    }

    /// Handle a single request/response exchange on a bidirectional stream.
    ///
    /// Reads a `FileRequest`, processes it, and writes the response.
    pub async fn handle_request<S, R>(&self, send: &mut S, recv: &mut R) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
        R: AsyncReadExt + Unpin,
    {
        // Read request
        let req_len = recv
            .read_u32()
            .await
            .context("failed to read request length")?;
        let mut req_buf = vec![0u8; req_len as usize];
        recv.read_exact(&mut req_buf)
            .await
            .context("failed to read request")?;
        let request = decode_file_request(&req_buf).context("failed to decode request")?;

        match request {
            FileRequest::GetBitfield { file_hash } => {
                tracing::debug!(
                    file_hash = data_encoding::HEXLOWER.encode(&file_hash),
                    "serving bitfield request"
                );
                let bitfield = self.store.get_chunk_bitfield(&file_hash)?;
                let resp = postcard::to_allocvec(&bitfield).context("failed to encode bitfield")?;
                send.write_u32(resp.len() as u32).await?;
                send.write_all(&resp).await?;
                send.flush().await?;
            }
            FileRequest::GetChunks {
                file_hash,
                chunk_indices,
            } => {
                tracing::debug!(
                    file_hash = data_encoding::HEXLOWER.encode(&file_hash),
                    count = chunk_indices.len(),
                    "serving chunk request"
                );

                // Verify we have the file and requested chunks
                if !self.store.is_complete(&file_hash)? {
                    tracing::warn!("refusing to serve incomplete file");
                    // Send zero-length response to signal error
                    send.write_u32(0).await?;
                    send.flush().await?;
                    return Ok(());
                }

                let file_path = self.store.file_path(&file_hash)?;
                let file_data = tokio::fs::read(&file_path)
                    .await
                    .with_context(|| format!("failed to read file: {}", file_path.display()))?;

                // Send chunk count header
                send.write_u32(chunk_indices.len() as u32).await?;

                for idx in &chunk_indices {
                    let start = *idx as usize * CHUNK_SIZE as usize;
                    let end = std::cmp::min(start + CHUNK_SIZE as usize, file_data.len());
                    if start >= file_data.len() {
                        tracing::warn!(chunk_index = idx, "requested chunk out of range");
                        continue;
                    }

                    let chunk = FileChunk {
                        file_hash,
                        chunk_index: *idx,
                        data: file_data[start..end].to_vec(),
                    };

                    let chunk_bytes =
                        encode_file_chunk(&chunk).context("failed to encode chunk")?;
                    send.write_u32(chunk_bytes.len() as u32).await?;
                    send.write_all(&chunk_bytes).await?;

                    self.metrics.chunks_served.fetch_add(1, Ordering::Relaxed);
                    self.metrics
                        .bytes_served
                        .fetch_add(chunk.data.len() as u64, Ordering::Relaxed);
                }

                send.flush().await?;
            }
            FileRequest::GetManifest { file_hash } => {
                tracing::debug!(
                    file_hash = data_encoding::HEXLOWER.encode(&file_hash),
                    "serving manifest request"
                );
                let manifest = self.store.get_file(&file_hash)?;
                let resp = postcard::to_allocvec(&manifest).context("failed to encode manifest")?;
                send.write_u32(resp.len() as u32).await?;
                send.write_all(&resp).await?;
                send.flush().await?;
            }
        }

        Ok(())
    }
}

/// A handle to a peer's stream for requesting chunks.
///
/// Generic over stream types to support both real connections and test doubles.
pub struct PeerStream<S, R> {
    pub send: S,
    pub recv: R,
    pub id: usize,
}

/// Downloads file chunks from multiple peers in parallel.
pub struct SwarmDownloader {
    metrics: Arc<SwarmMetrics>,
}

impl Default for SwarmDownloader {
    fn default() -> Self {
        Self::new()
    }
}

impl SwarmDownloader {
    pub fn new() -> Self {
        Self {
            metrics: Arc::default(),
        }
    }

    pub fn metrics(&self) -> &SwarmMetrics {
        &self.metrics
    }

    /// Query a peer's bitfield for a file.
    async fn query_bitfield<S, R>(
        send: &mut S,
        recv: &mut R,
        file_hash: &[u8; 32],
    ) -> Result<ChunkBitfield>
    where
        S: AsyncWriteExt + Unpin,
        R: AsyncReadExt + Unpin,
    {
        let req = FileRequest::GetBitfield {
            file_hash: *file_hash,
        };
        let req_bytes = encode_file_request(&req).context("failed to encode request")?;
        send.write_u32(req_bytes.len() as u32).await?;
        send.write_all(&req_bytes).await?;
        send.flush().await?;

        let resp_len = recv
            .read_u32()
            .await
            .context("failed to read bitfield response length")?;
        let mut resp_buf = vec![0u8; resp_len as usize];
        recv.read_exact(&mut resp_buf)
            .await
            .context("failed to read bitfield response")?;

        let bitfield: ChunkBitfield =
            postcard::from_bytes(&resp_buf).context("failed to decode bitfield")?;
        Ok(bitfield)
    }

    /// Request specific chunks from a peer.
    async fn request_chunks<S, R>(
        send: &mut S,
        recv: &mut R,
        file_hash: &[u8; 32],
        chunk_indices: Vec<u32>,
    ) -> Result<Vec<FileChunk>>
    where
        S: AsyncWriteExt + Unpin,
        R: AsyncReadExt + Unpin,
    {
        let req = FileRequest::GetChunks {
            file_hash: *file_hash,
            chunk_indices: chunk_indices.clone(),
        };
        let req_bytes = encode_file_request(&req).context("failed to encode request")?;
        send.write_u32(req_bytes.len() as u32).await?;
        send.write_all(&req_bytes).await?;
        send.flush().await?;

        // Read chunk count header
        let chunk_count = recv
            .read_u32()
            .await
            .context("failed to read chunk count")?;

        if chunk_count == 0 {
            anyhow::bail!("peer refused to serve chunks (file may be incomplete)");
        }

        let mut chunks = Vec::with_capacity(chunk_count as usize);
        for _ in 0..chunk_count {
            let chunk_len = recv
                .read_u32()
                .await
                .context("failed to read chunk length")?;
            let mut chunk_buf = vec![0u8; chunk_len as usize];
            recv.read_exact(&mut chunk_buf)
                .await
                .context("failed to read chunk data")?;
            let chunk = decode_file_chunk(&chunk_buf).context("failed to decode chunk")?;
            chunks.push(chunk);
        }

        Ok(chunks)
    }

    /// Download missing chunks of a file from available peers.
    ///
    /// Queries each peer's bitfield, then distributes chunk requests across peers
    /// that have the needed chunks.
    pub async fn download<S, R>(
        &self,
        file_hash: &[u8; 32],
        peers: &mut [PeerStream<S, R>],
        store: &FileStore,
    ) -> Result<()>
    where
        S: AsyncWriteExt + Unpin,
        R: AsyncReadExt + Unpin,
    {
        let manifest = store
            .get_file(file_hash)?
            .context("file not found in store")?;

        tracing::info!(
            file_hash = data_encoding::HEXLOWER.encode(file_hash),
            file_name = %manifest.file_name,
            chunk_count = manifest.chunk_count,
            "starting swarm download"
        );

        // Get our current bitfield to know which chunks we need
        let our_bitfield = store.get_chunk_bitfield(file_hash)?;
        let mut missing: Vec<u32> = (0..manifest.chunk_count)
            .filter(|i| !our_bitfield.has_chunk(*i))
            .collect();

        if missing.is_empty() {
            tracing::info!("all chunks already downloaded");
            return Ok(());
        }

        // Query each peer's bitfield
        let mut peer_bitfields: Vec<Option<ChunkBitfield>> = Vec::new();
        for peer in peers.iter_mut() {
            match Self::query_bitfield(&mut peer.send, &mut peer.recv, file_hash).await {
                Ok(bf) => {
                    tracing::debug!(peer_id = peer.id, "got bitfield from peer");
                    peer_bitfields.push(Some(bf));
                }
                Err(e) => {
                    tracing::warn!(peer_id = peer.id, error = %e, "failed to get bitfield from peer");
                    self.metrics.peer_failures.fetch_add(1, Ordering::Relaxed);
                    peer_bitfields.push(None);
                }
            }
        }

        // Assign chunks to peers round-robin, preferring peers that have the chunk
        while !missing.is_empty() {
            let mut progress = false;

            for (peer_idx, peer) in peers.iter_mut().enumerate() {
                let bitfield = match &peer_bitfields[peer_idx] {
                    Some(bf) => bf,
                    None => continue,
                };

                // Find chunks this peer has that we still need
                let available: Vec<u32> = missing
                    .iter()
                    .copied()
                    .filter(|idx| bitfield.has_chunk(*idx))
                    .collect();

                if available.is_empty() {
                    continue;
                }

                // Request a batch of chunks from this peer
                let batch: Vec<u32> = available.into_iter().take(8).collect();
                tracing::debug!(
                    peer_id = peer.id,
                    batch = ?batch,
                    "requesting chunks from peer"
                );

                match Self::request_chunks(&mut peer.send, &mut peer.recv, file_hash, batch.clone())
                    .await
                {
                    Ok(chunks) => {
                        let file_path = store.file_path(file_hash)?;
                        let mut file = tokio::fs::OpenOptions::new()
                            .create(true)
                            .write(true)
                            .truncate(false)
                            .open(&file_path)
                            .await?;

                        for chunk in &chunks {
                            // Write chunk to file at correct offset
                            let offset = chunk.chunk_index as u64 * manifest.chunk_size as u64;
                            use tokio::io::AsyncSeekExt;
                            file.seek(std::io::SeekFrom::Start(offset)).await?;
                            file.write_all(&chunk.data).await?;

                            store.set_chunk_received(file_hash, chunk.chunk_index)?;

                            self.metrics
                                .chunks_downloaded
                                .fetch_add(1, Ordering::Relaxed);
                            self.metrics
                                .bytes_downloaded
                                .fetch_add(chunk.data.len() as u64, Ordering::Relaxed);

                            // Remove from missing list
                            missing.retain(|idx| *idx != chunk.chunk_index);
                        }
                        progress = true;
                    }
                    Err(e) => {
                        tracing::warn!(
                            peer_id = peer.id,
                            error = %e,
                            "failed to download chunks from peer"
                        );
                        self.metrics.peer_failures.fetch_add(1, Ordering::Relaxed);
                        // Mark this peer as unavailable
                        peer_bitfields[peer_idx] = None;
                    }
                }
            }

            if !progress {
                anyhow::bail!(
                    "no peers available with missing chunks: {} chunks remaining",
                    missing.len()
                );
            }
        }

        // Verify SHA-256 of complete file
        let file_path = store.file_path(file_hash)?;
        let file_data = tokio::fs::read(&file_path).await?;
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest;
        hasher.update(&file_data);
        let computed = hasher.finalize();
        if computed.as_slice() != *file_hash {
            anyhow::bail!(
                "SHA-256 mismatch after download: expected {}, got {}",
                data_encoding::HEXLOWER.encode(file_hash),
                data_encoding::HEXLOWER.encode(computed.as_slice()),
            );
        }

        tracing::info!(
            file_hash = data_encoding::HEXLOWER.encode(file_hash),
            file_name = %manifest.file_name,
            "swarm download complete, SHA-256 verified"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::FileStore;
    use crate::transfer::CHUNK_SIZE;
    use sha2::Digest;
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

    /// Create a test file, add it to the store, and populate all chunks.
    async fn create_complete_file(
        store: &FileStore,
        name: &str,
        size: usize,
    ) -> ([u8; 32], Vec<u8>) {
        let data: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
        let mut hasher = sha2::Sha256::new();
        hasher.update(&data);
        let hash_result = hasher.finalize();
        let mut file_hash = [0u8; 32];
        file_hash.copy_from_slice(&hash_result);

        let chunk_count = (size as u64).div_ceil(CHUNK_SIZE as u64) as u32;
        let manifest = bisc_protocol::file::FileManifest {
            file_hash,
            file_name: name.to_string(),
            file_size: size as u64,
            chunk_size: CHUNK_SIZE,
            chunk_count,
        };

        store.add_file(&manifest).unwrap();
        let file_path = store.file_path(&file_hash).unwrap();
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&file_path, &data).await.unwrap();

        // Mark all chunks as received
        for i in 0..chunk_count {
            store.set_chunk_received(&file_hash, i).unwrap();
        }

        (file_hash, data)
    }

    /// Create an incomplete file entry in the store (metadata only, no chunks).
    fn create_incomplete_file(store: &FileStore, file_hash: [u8; 32], name: &str, size: usize) {
        let chunk_count = (size as u64).div_ceil(CHUNK_SIZE as u64) as u32;
        let manifest = bisc_protocol::file::FileManifest {
            file_hash,
            file_name: name.to_string(),
            file_size: size as u64,
            chunk_size: CHUNK_SIZE,
            chunk_count,
        };
        store.add_file(&manifest).unwrap();
    }

    /// Run a ChunkServer on one end of a duplex and return the other end.
    fn spawn_chunk_server(
        store: Arc<FileStore>,
    ) -> (
        tokio::io::WriteHalf<tokio::io::DuplexStream>,
        tokio::io::ReadHalf<tokio::io::DuplexStream>,
        tokio::task::JoinHandle<()>,
    ) {
        let (client, server) = tokio::io::duplex(1024 * 1024);
        let (client_read, client_write) = tokio::io::split(client);
        let (mut server_read, mut server_write) = tokio::io::split(server);

        let metrics = Arc::default();
        let chunk_server = ChunkServer::new(store, metrics);

        let handle = tokio::spawn(async move {
            // Handle requests in a loop until the connection closes
            loop {
                match chunk_server
                    .handle_request(&mut server_write, &mut server_read)
                    .await
                {
                    Ok(()) => {}
                    Err(_) => break,
                }
            }
        });

        (client_write, client_read, handle)
    }

    #[test]
    fn chunk_server_refuses_incomplete_file() {
        init_test_tracing();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store_dir = TempDir::new().unwrap();
            let store = Arc::new(FileStore::new(store_dir.path().to_path_buf()).unwrap());

            // Add file metadata but no chunks
            create_incomplete_file(&store, [0xAA; 32], "incomplete.bin", 500_000);

            let (mut client_write, mut client_read, _handle) = spawn_chunk_server(store);

            // Request chunks
            let req = FileRequest::GetChunks {
                file_hash: [0xAA; 32],
                chunk_indices: vec![0, 1],
            };
            let req_bytes = encode_file_request(&req).unwrap();
            client_write
                .write_u32(req_bytes.len() as u32)
                .await
                .unwrap();
            client_write.write_all(&req_bytes).await.unwrap();
            client_write.flush().await.unwrap();

            // Should get chunk_count = 0 (refusal)
            let chunk_count = client_read.read_u32().await.unwrap();
            assert_eq!(chunk_count, 0, "server should refuse incomplete files");
        });
    }

    #[tokio::test]
    async fn download_from_single_peer() {
        init_test_tracing();
        let server_dir = TempDir::new().unwrap();
        let client_dir = TempDir::new().unwrap();

        let server_store = Arc::new(FileStore::new(server_dir.path().to_path_buf()).unwrap());
        let client_store = FileStore::new(client_dir.path().to_path_buf()).unwrap();

        // Server has a complete file
        let (file_hash, original_data) =
            create_complete_file(&server_store, "test.bin", 500_000).await;

        // Client knows about the file but has no chunks
        create_incomplete_file(&client_store, file_hash, "test.bin", 500_000);

        // Create the output file directory
        let file_path = client_store.file_path(&file_hash).unwrap();
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        // Create empty file
        tokio::fs::write(&file_path, &[]).await.unwrap();

        let (client_write, client_read, _server_handle) = spawn_chunk_server(server_store);

        let downloader = SwarmDownloader::new();
        let mut peers = vec![PeerStream {
            send: client_write,
            recv: client_read,
            id: 0,
        }];

        downloader
            .download(&file_hash, &mut peers, &client_store)
            .await
            .unwrap();

        // Verify download
        assert!(client_store.is_complete(&file_hash).unwrap());
        let downloaded = tokio::fs::read(&file_path).await.unwrap();
        assert_eq!(downloaded, original_data);

        assert!(
            downloader
                .metrics()
                .chunks_downloaded
                .load(Ordering::Relaxed)
                > 0
        );
    }

    #[tokio::test]
    async fn download_from_multiple_peers() {
        init_test_tracing();
        let server1_dir = TempDir::new().unwrap();
        let server2_dir = TempDir::new().unwrap();
        let client_dir = TempDir::new().unwrap();

        let server1_store = Arc::new(FileStore::new(server1_dir.path().to_path_buf()).unwrap());
        let server2_store = Arc::new(FileStore::new(server2_dir.path().to_path_buf()).unwrap());
        let client_store = FileStore::new(client_dir.path().to_path_buf()).unwrap();

        // Both servers have the complete file
        let (file_hash, original_data) =
            create_complete_file(&server1_store, "shared.bin", 1_000_000).await;
        create_complete_file(&server2_store, "shared.bin", 1_000_000).await;

        // Client knows about the file but has no chunks
        create_incomplete_file(&client_store, file_hash, "shared.bin", 1_000_000);
        let file_path = client_store.file_path(&file_hash).unwrap();
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&file_path, &[]).await.unwrap();

        let (w1, r1, _h1) = spawn_chunk_server(server1_store);
        let (w2, r2, _h2) = spawn_chunk_server(server2_store);

        let downloader = SwarmDownloader::new();
        let mut peers = vec![
            PeerStream {
                send: w1,
                recv: r1,
                id: 0,
            },
            PeerStream {
                send: w2,
                recv: r2,
                id: 1,
            },
        ];

        downloader
            .download(&file_hash, &mut peers, &client_store)
            .await
            .unwrap();

        assert!(client_store.is_complete(&file_hash).unwrap());
        let downloaded = tokio::fs::read(&file_path).await.unwrap();
        assert_eq!(downloaded, original_data);
    }

    #[tokio::test]
    async fn download_resumes_after_peer_disconnect() {
        init_test_tracing();
        let server1_dir = TempDir::new().unwrap();
        let server2_dir = TempDir::new().unwrap();
        let client_dir = TempDir::new().unwrap();

        let server1_store = Arc::new(FileStore::new(server1_dir.path().to_path_buf()).unwrap());
        let server2_store = Arc::new(FileStore::new(server2_dir.path().to_path_buf()).unwrap());
        let client_store = FileStore::new(client_dir.path().to_path_buf()).unwrap();

        // Server 2 has the complete file, server 1 will be a broken/closed peer
        let (file_hash, original_data) =
            create_complete_file(&server2_store, "resume.bin", 500_000).await;

        create_incomplete_file(&client_store, file_hash, "resume.bin", 500_000);
        let file_path = client_store.file_path(&file_hash).unwrap();
        if let Some(parent) = file_path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&file_path, &[]).await.unwrap();

        // Server 1: immediately close the stream to simulate disconnect
        let (client1, server1) = tokio::io::duplex(1024);
        let (client1_read, client1_write) = tokio::io::split(client1);
        drop(server1); // Close immediately

        // Server 2: working server
        let (w2, r2, _h2) = spawn_chunk_server(server2_store);

        let downloader = SwarmDownloader::new();
        let mut peers = vec![
            PeerStream {
                send: client1_write,
                recv: client1_read,
                id: 0,
            },
            PeerStream {
                send: w2,
                recv: r2,
                id: 1,
            },
        ];

        downloader
            .download(&file_hash, &mut peers, &client_store)
            .await
            .unwrap();

        assert!(client_store.is_complete(&file_hash).unwrap());
        let downloaded = tokio::fs::read(&file_path).await.unwrap();
        assert_eq!(downloaded, original_data);

        // Peer 0 should have failed
        assert!(downloader.metrics().peer_failures.load(Ordering::Relaxed) >= 1);
    }
}
