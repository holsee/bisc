//! File sharing: 3 peers — A shares file, B downloads, A leaves, C downloads from B.

use std::io::Write;
use std::time::Duration;

use bisc_files::store::FileStore;
use bisc_files::transfer::{FileReceiver, FileSender};
use bisc_net::channel::Channel;

use crate::helpers::{
    init_test_tracing, setup_peer_with_media, wait_for_connection, wait_for_peers,
};

/// Three-peer file sharing scenario: A shares → B downloads → A leaves → C downloads from B.
#[tokio::test]
async fn three_peer_file_sharing() {
    init_test_tracing();

    let tmp = tempfile::TempDir::new().unwrap();

    // Create a test file (4KB)
    let file_dir = tmp.path().join("source");
    std::fs::create_dir_all(&file_dir).unwrap();
    let test_file = file_dir.join("test_document.txt");
    {
        let mut f = std::fs::File::create(&test_file).unwrap();
        let data: Vec<u8> = (0..4096).map(|i| (i % 256) as u8).collect();
        f.write_all(&data).unwrap();
    }

    // Setup 3 peers with media protocol (needed for direct QUIC connections)
    let (ep_a, gossip_a, _router_a, incoming_a) = setup_peer_with_media().await;
    let (mut channel_a, ticket) = Channel::create(
        ep_a.endpoint(),
        &gossip_a,
        "alice".to_string(),
        Some(incoming_a),
    )
    .await
    .unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let (ep_b, gossip_b, _router_b, incoming_b) = setup_peer_with_media().await;
    let mut channel_b = Channel::join(
        ep_b.endpoint(),
        &gossip_b,
        &ticket,
        "bob".to_string(),
        Some(incoming_b),
    )
    .await
    .unwrap();

    wait_for_peers(&mut channel_a, 1, 20).await;
    wait_for_peers(&mut channel_b, 1, 20).await;

    let a_id = channel_a.our_endpoint_id();
    let b_id = channel_b.our_endpoint_id();

    wait_for_connection(&mut channel_a, b_id, 20).await;
    wait_for_connection(&mut channel_b, a_id, 20).await;

    let peer_conn_a_to_b = channel_a.connection(&b_id).await.unwrap();
    let peer_conn_b_from_a = channel_b.connection(&a_id).await.unwrap();

    // --- Phase 1: A sends file to B ---
    let store_b = FileStore::new(tmp.path().join("store_b")).unwrap();

    let send_path = test_file.clone();
    let sender_conn = peer_conn_a_to_b.connection().clone();
    let receiver_conn = peer_conn_b_from_a.connection().clone();

    // A opens bi-stream, B accepts
    let sender_handle = tokio::spawn(async move {
        let (mut send, mut recv) = sender_conn.open_bi().await.unwrap();
        FileSender::default()
            .send_file(&send_path, &mut send, &mut recv)
            .await
            .unwrap()
    });

    let receiver_handle = tokio::spawn(async move {
        let (mut send, mut recv) = receiver_conn.accept_bi().await.unwrap();
        FileReceiver::default()
            .receive_file(&mut send, &mut recv, &store_b)
            .await
            .unwrap()
    });

    let sent_manifest = tokio::time::timeout(Duration::from_secs(10), sender_handle)
        .await
        .expect("sender timed out")
        .expect("sender panicked");
    let recv_manifest = tokio::time::timeout(Duration::from_secs(10), receiver_handle)
        .await
        .expect("receiver timed out")
        .expect("receiver panicked");

    assert_eq!(sent_manifest.file_hash, recv_manifest.file_hash);
    assert_eq!(recv_manifest.file_name, "test_document.txt");
    assert_eq!(recv_manifest.file_size, 4096);

    tracing::info!("Phase 1: A→B file transfer complete");

    // Verify B has the file
    let store_b = FileStore::new(tmp.path().join("store_b")).unwrap();
    let stored = store_b.get_file(&recv_manifest.file_hash).unwrap();
    assert!(stored.is_some(), "B should have the file in store");

    // --- Phase 2: A leaves, C joins via B's refreshed ticket ---
    let a_id_for_leave = channel_a.our_endpoint_id();
    channel_a.leave();
    ep_a.close().await;
    tracing::info!("A has left the channel");

    // Wait for B to detect A's departure before refreshing ticket
    crate::helpers::wait_for_peer_left(&mut channel_b, a_id_for_leave, 20).await;
    tracing::info!("B detected A left");

    // B refreshes the ticket so C can bootstrap from B's address
    let refreshed_ticket = channel_b.refresh_ticket();

    // C joins via B's refreshed ticket
    let (ep_c, gossip_c, _router_c, incoming_c) = setup_peer_with_media().await;
    let mut channel_c = Channel::join(
        ep_c.endpoint(),
        &gossip_c,
        &refreshed_ticket,
        "charlie".to_string(),
        Some(incoming_c),
    )
    .await
    .unwrap();

    wait_for_peers(&mut channel_b, 1, 20).await;
    wait_for_peers(&mut channel_c, 1, 20).await;

    let b_id = channel_b.our_endpoint_id();
    let c_id = channel_c.our_endpoint_id();

    wait_for_connection(&mut channel_b, c_id, 20).await;
    wait_for_connection(&mut channel_c, b_id, 20).await;

    let peer_conn_b_to_c = channel_b.connection(&c_id).await.unwrap();
    let peer_conn_c_from_b = channel_c.connection(&b_id).await.unwrap();

    // --- Phase 3: B sends file to C ---
    let store_c = FileStore::new(tmp.path().join("store_c")).unwrap();
    let b_file_path = store_b.file_path(&recv_manifest.file_hash).unwrap();

    let sender_conn = peer_conn_b_to_c.connection().clone();
    let receiver_conn = peer_conn_c_from_b.connection().clone();

    let sender_handle = tokio::spawn(async move {
        let (mut send, mut recv) = sender_conn.open_bi().await.unwrap();
        FileSender::default()
            .send_file(&b_file_path, &mut send, &mut recv)
            .await
            .unwrap()
    });

    let receiver_handle = tokio::spawn(async move {
        let (mut send, mut recv) = receiver_conn.accept_bi().await.unwrap();
        FileReceiver::default()
            .receive_file(&mut send, &mut recv, &store_c)
            .await
            .unwrap()
    });

    let sent_manifest_2 = tokio::time::timeout(Duration::from_secs(10), sender_handle)
        .await
        .expect("sender timed out")
        .expect("sender panicked");
    let recv_manifest_2 = tokio::time::timeout(Duration::from_secs(10), receiver_handle)
        .await
        .expect("receiver timed out")
        .expect("receiver panicked");

    assert_eq!(sent_manifest_2.file_hash, recv_manifest_2.file_hash);
    assert_eq!(
        recv_manifest_2.file_hash, recv_manifest.file_hash,
        "C should have the same file as B"
    );

    tracing::info!("Phase 3: B→C file transfer complete (A was gone)");

    // Verify C has the file
    let store_c = FileStore::new(tmp.path().join("store_c")).unwrap();
    let stored_c = store_c.get_file(&recv_manifest_2.file_hash).unwrap();
    assert!(stored_c.is_some(), "C should have the file in store");

    // Cleanup
    channel_b.leave();
    channel_c.leave();
    ep_b.close().await;
    ep_c.close().await;
}
