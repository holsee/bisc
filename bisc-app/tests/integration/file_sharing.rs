//! File sharing via iroh-blobs: A shares file, B downloads using iroh-blobs protocol.

use std::time::Duration;

use bisc_files::store::FileStore;
use bisc_net::channel::Channel;
use iroh_blobs::store::mem::MemStore;

use crate::helpers::{
    init_test_tracing, setup_peer_with_all_protocols, wait_for_connection, wait_for_peers,
    TestTimer, CONNECTION_TIMEOUT_SECS, PEER_DISCOVERY_TIMEOUT_SECS,
};

/// Two-peer file sharing via iroh-blobs: A adds file to blob store, B downloads via ALPN.
#[tokio::test]
async fn two_peer_iroh_blobs_file_sharing() {
    init_test_tracing();
    let mut timer = TestTimer::new("two_peer_iroh_blobs_file_sharing");

    let tmp = tempfile::TempDir::new().unwrap();

    // Create a test file
    let file_dir = tmp.path().join("source");
    std::fs::create_dir_all(&file_dir).unwrap();
    let test_file = file_dir.join("test_document.txt");
    let test_data: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
    tokio::fs::write(&test_file, &test_data).await.unwrap();

    // Setup peer A with blob store and all protocols
    let blob_store_a = MemStore::new();
    let (ep_a, gossip_a, _router_a, incoming_a) =
        setup_peer_with_all_protocols(&blob_store_a).await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Setup peer B with blob store and all protocols
    let blob_store_b = MemStore::new();
    let (ep_b, gossip_b, _router_b, incoming_b) =
        setup_peer_with_all_protocols(&blob_store_b).await;
    let mut channel_b = Channel::join(
        ep_b.endpoint(),
        &gossip_b,
        &ticket,
        "bob".to_string(),
        Some(incoming_b),
    )
    .await
    .unwrap();

    wait_for_peers(&mut channel_a, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;
    wait_for_peers(&mut channel_b, 1, PEER_DISCOVERY_TIMEOUT_SECS).await;

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    wait_for_connection(&mut channel_a, b_id, CONNECTION_TIMEOUT_SECS).await;
    wait_for_connection(&mut channel_b, a_id, CONNECTION_TIMEOUT_SECS).await;
    timer.phase("setup");

    // --- Phase 1: A adds file to blob store ---
    let tag_info = blob_store_a
        .blobs()
        .add_path(&test_file)
        .await
        .expect("failed to add file to blob store");
    let file_hash: [u8; 32] = *tag_info.hash.as_bytes();

    // Register in metadata store
    let store_a = FileStore::new(tmp.path().join("store_a")).unwrap();
    let manifest = bisc_protocol::file::FileManifest {
        file_hash,
        file_name: "test_document.txt".to_string(),
        file_size: 4096,
    };
    store_a.add_file(&manifest).unwrap();
    store_a.set_complete(&file_hash).unwrap();
    timer.phase("share");

    tracing::info!(
        file_hash = data_encoding::HEXLOWER.encode(&file_hash),
        "file shared by A"
    );

    // --- Phase 2: B downloads file from A via iroh-blobs ---
    let a_iroh_id = ep_a.endpoint().id();
    let hash = iroh_blobs::Hash::from_bytes(file_hash);

    let connection = ep_b
        .endpoint()
        .connect(a_iroh_id, iroh_blobs::ALPN)
        .await
        .expect("failed to connect to peer A for blob download");

    let request = iroh_blobs::protocol::GetRequest::blob(hash);
    blob_store_b
        .remote()
        .execute_get(connection, request)
        .await
        .expect("blob download failed");
    timer.phase("download");

    tracing::info!("B downloaded file from A");

    // Verify B has the blob
    let has_blob = blob_store_b.blobs().has(hash).await.unwrap();
    assert!(has_blob, "B should have the blob after download");

    // --- Phase 3: Export and verify file contents ---
    let export_path = tmp.path().join("exported.txt");
    let exported_size = blob_store_b
        .blobs()
        .export(hash, &export_path)
        .await
        .expect("failed to export blob");

    assert_eq!(exported_size, 4096);

    let exported_data = tokio::fs::read(&export_path).await.unwrap();
    assert_eq!(
        exported_data, test_data,
        "exported file should match original"
    );
    timer.phase("verify");

    tracing::info!("File verified — content matches");

    // Cleanup
    channel_a.leave();
    channel_b.leave();
    ep_a.close().await;
    ep_b.close().await;
}
