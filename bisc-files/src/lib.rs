//! File storage and transfer via iroh-blobs.
//!
//! Uses iroh-blobs for content-addressed BLAKE3 verified streaming.
//! A thin SQLite metadata layer tracks file names and sender info for the UI.

pub mod store;
