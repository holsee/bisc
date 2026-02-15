//! File metadata store: thin SQLite layer tracking file names, sizes, and
//! BLAKE3 hashes for UI display. Actual file data is stored and transferred
//! by iroh-blobs.

use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use bisc_protocol::file::FileManifest;
use rusqlite::Connection;

/// Manages file metadata in SQLite.
///
/// Thread-safe: the inner SQLite connection is protected by a `Mutex`.
pub struct FileStore {
    storage_dir: PathBuf,
    conn: Mutex<Connection>,
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
                complete INTEGER NOT NULL DEFAULT 0
            );",
        )
        .context("failed to initialize database schema")?;

        tracing::info!(storage_dir = %storage_dir.display(), "file store opened");

        Ok(Self {
            storage_dir,
            conn: Mutex::new(conn),
        })
    }

    /// Add a file manifest to the metadata store.
    pub fn add_file(&self, manifest: &FileManifest) -> Result<[u8; 32]> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO files (hash, name, size, complete)
                 VALUES (?1, ?2, ?3, 0)",
            rusqlite::params![
                manifest.file_hash.as_slice(),
                manifest.file_name,
                manifest.file_size as i64,
            ],
        )
        .context("failed to insert file")?;

        tracing::info!(
            file_hash = data_encoding::HEXLOWER.encode(&manifest.file_hash),
            file_name = %manifest.file_name,
            file_size = manifest.file_size,
            "file added to metadata store"
        );

        Ok(manifest.file_hash)
    }

    /// Get a file manifest by hash.
    pub fn get_file(&self, file_hash: &[u8; 32]) -> Result<Option<FileManifest>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT hash, name, size FROM files WHERE hash = ?1")
            .context("failed to prepare query")?;

        let result = stmt.query_row(rusqlite::params![file_hash.as_slice()], |row| {
            let hash_vec: Vec<u8> = row.get(0)?;
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&hash_vec);
            Ok(FileManifest {
                file_hash: hash,
                file_name: row.get(1)?,
                file_size: row.get::<_, i64>(2)? as u64,
            })
        });

        match result {
            Ok(manifest) => Ok(Some(manifest)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e).context("failed to query file"),
        }
    }

    /// Mark a file as completely downloaded.
    pub fn set_complete(&self, file_hash: &[u8; 32]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE files SET complete = 1 WHERE hash = ?1",
            rusqlite::params![file_hash.as_slice()],
        )
        .context("failed to mark file complete")?;

        tracing::info!(
            file_hash = data_encoding::HEXLOWER.encode(file_hash),
            "file marked complete"
        );
        Ok(())
    }

    /// Check if a file has been fully downloaded.
    pub fn is_complete(&self, file_hash: &[u8; 32]) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let complete: i64 = conn
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT hash, name, size FROM files")
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
                })
            })
            .context("failed to query files")?;

        let mut result = Vec::new();
        for file in files {
            result.push(file.context("failed to read file row")?);
        }

        Ok(result)
    }

    /// Get the storage directory root.
    pub fn storage_dir(&self) -> &PathBuf {
        &self.storage_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_manifest(hash_byte: u8) -> FileManifest {
        FileManifest {
            file_hash: [hash_byte; 32],
            file_name: format!("test_{hash_byte:02x}.dat"),
            file_size: 1_000_000,
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

        let manifest = test_manifest(0xAA);
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
    fn set_complete_marks_file() {
        let tmp = TempDir::new().unwrap();
        let store = FileStore::new(tmp.path().to_path_buf()).unwrap();

        let manifest = test_manifest(0xBB);
        store.add_file(&manifest).unwrap();

        assert!(!store.is_complete(&manifest.file_hash).unwrap());
        store.set_complete(&manifest.file_hash).unwrap();
        assert!(store.is_complete(&manifest.file_hash).unwrap());
    }

    #[test]
    fn list_files_returns_all_added() {
        let tmp = TempDir::new().unwrap();
        let store = FileStore::new(tmp.path().to_path_buf()).unwrap();

        let m1 = test_manifest(0x01);
        let m2 = test_manifest(0x02);
        let m3 = test_manifest(0x03);

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
        let manifest = test_manifest(0xDD);

        {
            let store = FileStore::new(store_dir.clone()).unwrap();
            store.add_file(&manifest).unwrap();
            store.set_complete(&manifest.file_hash).unwrap();
        }

        {
            let store = FileStore::new(store_dir).unwrap();
            let retrieved = store.get_file(&manifest.file_hash).unwrap();
            assert_eq!(retrieved, Some(manifest.clone()));
            assert!(store.is_complete(&manifest.file_hash).unwrap());
        }
    }
}
