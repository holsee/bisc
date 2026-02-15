//! File sharing protocol types.
//!
//! File hashes are BLAKE3 (32 bytes), computed by iroh-blobs.

use serde::{Deserialize, Serialize};

/// Metadata about a shared file.
///
/// The `file_hash` is a BLAKE3 hash produced by iroh-blobs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileManifest {
    pub file_hash: [u8; 32],
    pub file_name: String,
    pub file_size: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_manifest_roundtrip() {
        let manifest = FileManifest {
            file_hash: [0xDE; 32],
            file_name: "photo.jpg".to_string(),
            file_size: 2_000_000,
        };
        let encoded: Vec<u8> = postcard::to_allocvec(&manifest).unwrap();
        let decoded: FileManifest = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(manifest, decoded);
    }
}
