//! File sharing protocol types.

use serde::{Deserialize, Serialize};

use crate::types::EndpointId;

/// Metadata about a shared file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileManifest {
    pub file_hash: [u8; 32],
    pub file_name: String,
    pub file_size: u64,
    pub chunk_size: u32,
    pub chunk_count: u32,
}

/// Request to download a file or specific chunks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileRequest {
    /// Request the full file manifest.
    GetManifest { file_hash: [u8; 32] },
    /// Request specific chunks.
    GetChunks {
        file_hash: [u8; 32],
        chunk_indices: Vec<u32>,
    },
    /// Query which chunks a peer has.
    GetBitfield { file_hash: [u8; 32] },
}

/// A single chunk of file data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChunk {
    pub file_hash: [u8; 32],
    pub chunk_index: u32,
    pub data: Vec<u8>,
}

/// Bitfield tracking which chunks a peer has for a given file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkBitfield {
    pub file_hash: [u8; 32],
    pub peer_id: EndpointId,
    /// Each bit represents a chunk. Bit N = 1 means chunk N is available.
    pub bitfield: Vec<u8>,
}

impl ChunkBitfield {
    /// Create a new empty bitfield for the given number of chunks.
    pub fn new(file_hash: [u8; 32], peer_id: EndpointId, chunk_count: u32) -> Self {
        let byte_count = (chunk_count as usize).div_ceil(8);
        Self {
            file_hash,
            peer_id,
            bitfield: vec![0; byte_count],
        }
    }

    /// Check if a specific chunk is available.
    pub fn has_chunk(&self, index: u32) -> bool {
        let byte_idx = index as usize / 8;
        let bit_idx = index as usize % 8;
        if byte_idx >= self.bitfield.len() {
            return false;
        }
        self.bitfield[byte_idx] & (1 << bit_idx) != 0
    }

    /// Mark a specific chunk as available.
    pub fn set_chunk(&mut self, index: u32) {
        let byte_idx = index as usize / 8;
        let bit_idx = index as usize % 8;
        if byte_idx < self.bitfield.len() {
            self.bitfield[byte_idx] |= 1 << bit_idx;
        }
    }
}

/// Serialize a `FileRequest` to compact binary via postcard.
pub fn encode_file_request(req: &FileRequest) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(req)
}

/// Deserialize a `FileRequest` from postcard bytes.
pub fn decode_file_request(data: &[u8]) -> Result<FileRequest, postcard::Error> {
    postcard::from_bytes(data)
}

/// Serialize a `FileChunk` to compact binary via postcard.
pub fn encode_file_chunk(chunk: &FileChunk) -> Result<Vec<u8>, postcard::Error> {
    postcard::to_allocvec(chunk)
}

/// Deserialize a `FileChunk` from postcard bytes.
pub fn decode_file_chunk(data: &[u8]) -> Result<FileChunk, postcard::Error> {
    postcard::from_bytes(data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::EndpointId;

    #[test]
    fn file_manifest_roundtrip() {
        let manifest = FileManifest {
            file_hash: [0xDE; 32],
            file_name: "photo.jpg".to_string(),
            file_size: 2_000_000,
            chunk_size: 262_144,
            chunk_count: 8,
        };
        let encoded: Vec<u8> = postcard::to_allocvec(&manifest).unwrap();
        let decoded: FileManifest = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(manifest, decoded);
    }

    #[test]
    fn file_request_get_manifest_roundtrip() {
        let req = FileRequest::GetManifest {
            file_hash: [0xAA; 32],
        };
        let encoded = encode_file_request(&req).unwrap();
        let decoded = decode_file_request(&encoded).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn file_request_get_chunks_roundtrip() {
        let req = FileRequest::GetChunks {
            file_hash: [0xBB; 32],
            chunk_indices: vec![0, 2, 5, 7],
        };
        let encoded = encode_file_request(&req).unwrap();
        let decoded = decode_file_request(&encoded).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn file_request_get_bitfield_roundtrip() {
        let req = FileRequest::GetBitfield {
            file_hash: [0xCC; 32],
        };
        let encoded = encode_file_request(&req).unwrap();
        let decoded = decode_file_request(&encoded).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn file_chunk_roundtrip() {
        let chunk = FileChunk {
            file_hash: [0xDD; 32],
            chunk_index: 3,
            data: vec![1, 2, 3, 4, 5],
        };
        let encoded = encode_file_chunk(&chunk).unwrap();
        let decoded = decode_file_chunk(&encoded).unwrap();
        assert_eq!(chunk, decoded);
    }

    #[test]
    fn chunk_bitfield_operations() {
        let mut bf = ChunkBitfield::new([0; 32], EndpointId([1; 32]), 16);
        assert!(!bf.has_chunk(0));
        assert!(!bf.has_chunk(15));

        bf.set_chunk(0);
        bf.set_chunk(7);
        bf.set_chunk(15);

        assert!(bf.has_chunk(0));
        assert!(bf.has_chunk(7));
        assert!(bf.has_chunk(15));
        assert!(!bf.has_chunk(1));
        assert!(!bf.has_chunk(8));
    }

    #[test]
    fn chunk_bitfield_roundtrip() {
        let mut bf = ChunkBitfield::new([0xFF; 32], EndpointId([2; 32]), 32);
        bf.set_chunk(0);
        bf.set_chunk(10);
        bf.set_chunk(31);

        let encoded: Vec<u8> = postcard::to_allocvec(&bf).unwrap();
        let decoded: ChunkBitfield = postcard::from_bytes(&encoded).unwrap();
        assert_eq!(bf, decoded);
        assert!(decoded.has_chunk(0));
        assert!(decoded.has_chunk(10));
        assert!(decoded.has_chunk(31));
        assert!(!decoded.has_chunk(1));
    }
}
