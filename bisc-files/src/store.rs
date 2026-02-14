//! File storage: manages the local storage directory and SQLite metadata database.
//!
//! Each file is stored at `<storage_dir>/bisc/<hex_hash>/<filename>`.
//! SQLite tracks file metadata (manifests) and chunk download progress.

use std::path::PathBuf;

use anyhow::{Context, Result};
use bisc_protocol::file::{ChunkBitfield, FileManifest};
use bisc_protocol::types::EndpointId;
use rusqlite::Connection;

/// Manages local file storage and SQLite metadata.
pub struct FileStore {
    storage_dir: PathBuf,
    conn: Connection,
}

impl FileStore {
    /// Create a new FileStore rooted at `storage_dir`.
    ///
    /// Creates the directory and SQLite database if they don't exist.
    pub fn new(storage_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&storage_dir).with_context(|| {
            format!(
                "failed to create storage directory: {}",
                storage_dir.display()
            )
        })?;

        let db_path = storage_dir.join("bisc.db");
        let conn = Connection::open(&db_path)
            .with_context(|| format!("failed to open database: {}", db_path.display()))?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS files (
                hash BLOB PRIMARY KEY,
                name TEXT NOT NULL,
                size INTEGER NOT NULL,
                chunk_size INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL,
                complete INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS chunks (
                file_hash BLOB NOT NULL,
                chunk_index INTEGER NOT NULL,
                received INTEGER NOT NULL DEFAULT 1,
                PRIMARY KEY (file_hash, chunk_index),
                FOREIGN KEY (file_hash) REFERENCES files(hash)
            );",
        )
        .context("failed to initialize database schema")?;

        tracing::info!(storage_dir = %storage_dir.display(), "file store opened");

        Ok(Self { storage_dir, conn })
    }

    /// Add a file manifest to the store. Returns the file hash.
    pub fn add_file(&self, manifest: &FileManifest) -> Result<[u8; 32]> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO files (hash, name, size, chunk_size, chunk_count, complete)
                 VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                rusqlite::params![
                    manifest.file_hash.as_slice(),
                    manifest.file_name,
                    manifest.file_size as i64,
                    manifest.chunk_size as i64,
                    manifest.chunk_count as i64,
                ],
            )
            .context("failed to insert file")?;

        let file_dir = self.file_dir(&manifest.file_hash);
        std::fs::create_dir_all(&file_dir).with_context(|| {
            format!(
                "failed to create file directory: {}",
                file_dir.display()
            )
        })?;

        tracing::info!(
            file_hash = data_encoding::HEXLOWER.encode(&manifest.file_hash),
            file_name = %manifest.file_name,
            file_size = manifest.file_size,
            chunk_count = manifest.chunk_count,
            "file added to store"
        );

        Ok(manifest.file_hash)
    }

    /// Get a file manifest by hash.
    pub fn get_file(&self, file_hash: &[u8; 32]) -> Result<Option<FileManifest>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT hash, name, size, chunk_size, chunk_count FROM files WHERE hash = ?1",
            )
            .context("failed to prepare query")?;

        let result = stmt.query_row(rusqlite::params![file_hash.as_slice()], |row| {
            let hash_vec: Vec<u8> = row.get(0)?;
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&hash_vec);
            Ok(FileManifest {
                file_hash: hash,
                file_name: row.get(1)?,
                file_size: row.get::<_, i64>(2)? as u64,
                chunk_size: row.get::<_, i64>(3)? as u32,
                chunk_count: row.get::<_, i64>(4)? as u32,
            })
        });

        match result {
            Ok(manifest) => Ok(Some(manifest)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to query file"),
        }
    }

    /// Mark a chunk as received.
    pub fn set_chunk_received(&self, file_hash: &[u8; 32], chunk_index: u32) -> Result<()> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO chunks (file_hash, chunk_index, received)
                 VALUES (?1, ?2, 1)",
                rusqlite::params![file_hash.as_slice(), chunk_index as i64],
            )
            .context("failed to mark chunk received")?;

        // Check if file is now complete and update the flag.
        let chunk_count: i64 = self
            .conn
            .query_row(
                "SELECT chunk_count FROM files WHERE hash = ?1",
                rusqlite::params![file_hash.as_slice()],
                |row| row.get(0),
            )
            .context("failed to query chunk count")?;

        let received_count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_hash = ?1 AND received = 1",
                rusqlite::params![file_hash.as_slice()],
                |row| row.get(0),
            )
            .context("failed to count received chunks")?;

        if received_count >= chunk_count {
            self.conn
                .execute(
                    "UPDATE files SET complete = 1 WHERE hash = ?1",
                    rusqlite::params![file_hash.as_slice()],
                )
                .context("failed to update file completeness")?;

            tracing::info!(
                file_hash = data_encoding::HEXLOWER.encode(file_hash),
                "file download complete"
            );
        }

        tracing::debug!(
            file_hash = data_encoding::HEXLOWER.encode(file_hash),
            chunk_index,
            received = received_count,
            total = chunk_count,
            "chunk received"
        );

        Ok(())
    }

    /// Get the chunk bitfield for a file.
    ///
    /// The returned bitfield uses a zero `EndpointId`; callers should set the
    /// peer ID if needed for protocol exchange.
    pub fn get_chunk_bitfield(&self, file_hash: &[u8; 32]) -> Result<ChunkBitfield> {
        let chunk_count: i64 = self
            .conn
            .query_row(
                "SELECT chunk_count FROM files WHERE hash = ?1",
                rusqlite::params![file_hash.as_slice()],
                |row| row.get(0),
            )
            .context("failed to query chunk count")?;

        let mut bitfield =
            ChunkBitfield::new(*file_hash, EndpointId([0; 32]), chunk_count as u32);

        let mut stmt = self
            .conn
            .prepare("SELECT chunk_index FROM chunks WHERE file_hash = ?1 AND received = 1")
            .context("failed to prepare bitfield query")?;

        let indices = stmt
            .query_map(rusqlite::params![file_hash.as_slice()], |row| {
                row.get::<_, i64>(0)
            })
            .context("failed to query chunks")?;

        for index in indices {
            let idx = index.context("failed to read chunk index")?;
            bitfield.set_chunk(idx as u32);
        }

        Ok(bitfield)
    }

    /// Check if all chunks for a file have been received.
    pub fn is_complete(&self, file_hash: &[u8; 32]) -> Result<bool> {
        let complete: i64 = self
            .conn
            .query_row(
                "SELECT complete FROM files WHERE hash = ?1",
                rusqlite::params![file_hash.as_slice()],
                |row| row.get(0),
            )
            .context("failed to query file completeness")?;

        Ok(complete != 0)
    }

    /// List all known files.
    pub fn list_files(&self) -> Result<Vec<FileManifest>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hash, name, size, chunk_size, chunk_count FROM files")
            .context("failed to prepare list query")?;

        let files = stmt
            .query_map([], |row| {
                let hash_vec: Vec<u8> = row.get(0)?;
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&hash_vec);
                Ok(FileManifest {
                    file_hash: hash,
                    file_name: row.get(1)?,
                    file_size: row.get::<_, i64>(2)? as u64,
                    chunk_size: row.get::<_, i64>(3)? as u32,
                    chunk_count: row.get::<_, i64>(4)? as u32,
                })
            })
            .context("failed to query files")?;

        let mut result = Vec::new();
        for file in files {
            result.push(file.context("failed to read file row")?);
        }

        Ok(result)
    }

    /// Get the storage path for a file.
    ///
    /// Returns `<storage_dir>/bisc/<hex_hash>/<filename>`.
    pub fn file_path(&self, file_hash: &[u8; 32]) -> Result<PathBuf> {
        let manifest = self.get_file(file_hash)?.context("file not found")?;
        Ok(self.file_dir(file_hash).join(&manifest.file_name))
    }

    /// Get the directory for a file's data.
    fn file_dir(&self, file_hash: &[u8; 32]) -> PathBuf {
        let hex = data_encoding::HEXLOWER.encode(file_hash);
        self.storage_dir.join("bisc").join(hex)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_manifest(hash_byte: u8, chunk_count: u32) -> FileManifest {
        FileManifest {
            file_hash: [hash_byte; 32],
            file_name: format!("test_{hash_byte:02x}.dat"),
            file_size: chunk_count as u64 * 262_144,
            chunk_size: 262_144,
            chunk_count,
        }
    }

    #[test]
    fn new_creates_directory_and_database() {
        let tmp = TempDir::new().unwrap();
        let store_dir = tmp.path().join("store");
        let _store = FileStore::new(store_dir.clone()).unwrap();

        assert!(store_dir.exists());
        assert!(store_dir.join("bisc.db").exists());
    }

    #[test]
    fn add_and_get_file_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let store = FileStore::new(tmp.path().to_path_buf()).unwrap();

        let manifest = test_manifest(0xAA, 8);
        store.add_file(&manifest).unwrap();

        let retrieved = store.get_file(&manifest.file_hash).unwrap();
        assert_eq!(retrieved, Some(manifest));
    }

    #[test]
    fn get_file_returns_none_for_unknown() {
        let tmp = TempDir::new().unwrap();
        let store = FileStore::new(tmp.path().to_path_buf()).unwrap();

        let result = store.get_file(&[0xFF; 32]).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn chunk_tracking_and_bitfield() {
        let tmp = TempDir::new().unwrap();
        let store = FileStore::new(tmp.path().to_path_buf()).unwrap();

        let manifest = test_manifest(0xBB, 10);
        store.add_file(&manifest).unwrap();

        // No chunks received initially
        let bf = store.get_chunk_bitfield(&manifest.file_hash).unwrap();
        for i in 0..10 {
            assert!(!bf.has_chunk(i), "chunk {i} should not be set");
        }

        // Receive some chunks
        store
            .set_chunk_received(&manifest.file_hash, 0)
            .unwrap();
        store
            .set_chunk_received(&manifest.file_hash, 3)
            .unwrap();
        store
            .set_chunk_received(&manifest.file_hash, 9)
            .unwrap();

        let bf = store.get_chunk_bitfield(&manifest.file_hash).unwrap();
        assert!(bf.has_chunk(0));
        assert!(!bf.has_chunk(1));
        assert!(!bf.has_chunk(2));
        assert!(bf.has_chunk(3));
        assert!(bf.has_chunk(9));

        // Idempotent: setting same chunk again should not error
        store
            .set_chunk_received(&manifest.file_hash, 0)
            .unwrap();
    }

    #[test]
    fn is_complete_only_when_all_chunks_received() {
        let tmp = TempDir::new().unwrap();
        let store = FileStore::new(tmp.path().to_path_buf()).unwrap();

        let manifest = test_manifest(0xCC, 3);
        store.add_file(&manifest).unwrap();

        assert!(!store.is_complete(&manifest.file_hash).unwrap());

        store
            .set_chunk_received(&manifest.file_hash, 0)
            .unwrap();
        assert!(!store.is_complete(&manifest.file_hash).unwrap());

        store
            .set_chunk_received(&manifest.file_hash, 1)
            .unwrap();
        assert!(!store.is_complete(&manifest.file_hash).unwrap());

        store
            .set_chunk_received(&manifest.file_hash, 2)
            .unwrap();
        assert!(store.is_complete(&manifest.file_hash).unwrap());
    }

    #[test]
    fn list_files_returns_all_added() {
        let tmp = TempDir::new().unwrap();
        let store = FileStore::new(tmp.path().to_path_buf()).unwrap();

        let m1 = test_manifest(0x01, 4);
        let m2 = test_manifest(0x02, 8);
        let m3 = test_manifest(0x03, 16);

        store.add_file(&m1).unwrap();
        store.add_file(&m2).unwrap();
        store.add_file(&m3).unwrap();

        let files = store.list_files().unwrap();
        assert_eq!(files.len(), 3);
        assert!(files.contains(&m1));
        assert!(files.contains(&m2));
        assert!(files.contains(&m3));
    }

    #[test]
    fn database_persists_across_instances() {
        let tmp = TempDir::new().unwrap();
        let store_dir = tmp.path().to_path_buf();
        let manifest = test_manifest(0xDD, 5);

        // First instance: add file and some chunks
        {
            let store = FileStore::new(store_dir.clone()).unwrap();
            store.add_file(&manifest).unwrap();
            store
                .set_chunk_received(&manifest.file_hash, 0)
                .unwrap();
            store
                .set_chunk_received(&manifest.file_hash, 2)
                .unwrap();
        }

        // Second instance: data should persist
        {
            let store = FileStore::new(store_dir).unwrap();
            let retrieved = store.get_file(&manifest.file_hash).unwrap();
            assert_eq!(retrieved, Some(manifest.clone()));

            let bf = store.get_chunk_bitfield(&manifest.file_hash).unwrap();
            assert!(bf.has_chunk(0));
            assert!(!bf.has_chunk(1));
            assert!(bf.has_chunk(2));

            assert!(!store.is_complete(&manifest.file_hash).unwrap());
        }
    }

    #[test]
    fn file_path_returns_correct_path() {
        let tmp = TempDir::new().unwrap();
        let store = FileStore::new(tmp.path().to_path_buf()).unwrap();

        let manifest = test_manifest(0xEE, 1);
        store.add_file(&manifest).unwrap();

        let path = store.file_path(&manifest.file_hash).unwrap();
        let hex = data_encoding::HEXLOWER.encode(&manifest.file_hash);
        let expected = tmp.path().join("bisc").join(&hex).join(&manifest.file_name);
        assert_eq!(path, expected);
    }
}
