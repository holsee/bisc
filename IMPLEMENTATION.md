# Implementation Recommendation

## Architecture: Pure Rust — Iced GUI + iroh P2P (Fully Decentralized)

```
+---------------------------------------------------------------+
|                     bisc (single binary)                       |
+---------------------------------------------------------------+
|                                                                |
|  +---------------------------+  +---------------------------+  |
|  |        Iced UI Layer      |  |     Media Engine          |  |
|  |                           |  |                           |  |
|  |  - Elm architecture       |  |  - Opus encoder/decoder   |  |
|  |  - wgpu GPU rendering     |  |  - H264/VP8 enc/dec      |  |
|  |  - Custom shader widget   |  |  - Jitter buffer          |  |
|  |    for video surfaces     |  |  - Frame packetization    |  |
|  |  - Channel join UI        |  |  - Congestion estimation  |  |
|  |  - Ticket display/scan    |  |  - Adaptive bitrate       |  |
|  +---------------------------+  +---------------------------+  |
|               |                            |                   |
|               +------- shared state -------+                   |
|                    (no IPC, no FFI,                             |
|                     same process)                              |
|                            |                                   |
|  +----------------------------------------------------------+ |
|  |                    Channel Manager                        | |
|  |                                                           | |
|  |  - Creates/joins channels via iroh-gossip topics          | |
|  |  - BiscTicket creation and parsing                        | |
|  |  - Peer membership tracking                               | |
|  |  - Triggers direct connections on peer discovery           | |
|  |  - Ticket refresh when bootstrap peer leaves              | |
|  +----------------------------------------------------------+ |
|                            |                                   |
|  +----------------------------------------------------------+ |
|  |                 iroh Networking Layer                      | |
|  |                                                           | |
|  |  +------------------+  +-------------------------------+  | |
|  |  | iroh Endpoint    |  | iroh-gossip                   |  | |
|  |  |                  |  |                               |  | |
|  |  | - NAT traversal  |  | - Channel topic pub/sub      |  | |
|  |  | - Hole punching  |  | - Peer announce/leave        |  | |
|  |  | - Relay fallback |  | - Media state broadcast      |  | |
|  |  | - QUIC conns     |  | - File announcements         |  | |
|  |  | - Datagrams      |  | - Heartbeats                 |  | |
|  |  +------------------+  +-------------------------------+  | |
|  |                                                           | |
|  |  Per-peer connections (full mesh):                        | |
|  |  +----------------------------------------------------+  | |
|  |  | QUIC Connection to Peer X                          |  | |
|  |  |                                                    |  | |
|  |  | Unreliable Datagrams: audio/video MediaPackets     |  | |
|  |  | Reliable Stream 0:    media control (PLI, stats)   |  | |
|  |  | Reliable Stream 1+:   file transfer chunks         |  | |
|  |  +----------------------------------------------------+  | |
|  +----------------------------------------------------------+ |
|                                                                |
|  +---------------------------+  +---------------------------+  |
|  |     Scoped Audio          |  |     File Sharing          |  |
|  |                           |  |                           |  |
|  |  - wasapi (Windows)       |  |  - Chunked transfer over  |  |
|  |  - ScreenCaptureKit       |  |    iroh reliable streams  |  |
|  |    (macOS via objc2)      |  |  - Peer-assisted download |  |
|  |  - pipewire-rs (Linux)    |  |  - rusqlite (metadata)    |  |
|  +---------------------------+  +---------------------------+  |
+---------------------------------------------------------------+
```

### Why This Architecture

**No central server.** There is no application server — no signaling server, no room server, no user database. Peers connect directly via iroh's P2P QUIC networking with built-in NAT traversal. A "channel" is an iroh-gossip topic that peers subscribe to.

**iroh replaces both WebRTC and the signaling server.** The original design used `str0m` (WebRTC) for media, which requires SDP/ICE exchange via a signaling server — contradicting the "no server" goal. iroh provides everything needed in one stack: NAT traversal, hole punching, relay fallback, encrypted QUIC connections with both reliable streams and unreliable datagrams. Media frames are sent as QUIC datagrams (same semantics as RTP over UDP). No SDP, no ICE, no signaling protocol.

**Single connection per peer pair.** One QUIC connection carries media datagrams, control messages, and file transfers simultaneously. No duplicate connections or transports.

**Truly self-contained single binary.** No webview runtime, no system rendering dependencies. GPU rendering via wgpu (Vulkan/Metal/DX12/OpenGL). Drop the binary on any machine and run it.

**Zero-copy video rendering.** Decoded video frames go directly from the decoder buffer to a wgpu GPU texture via Iced's custom shader widget. No IPC, no serialization, no process boundary.

**One language, one build system.** Rust end-to-end — UI, media engine, networking, file sharing. One `Cargo.toml`, one compile target.

---

## Decentralized Channel System

### How It Works

A "channel" is an `iroh-gossip` topic. Peers subscribe to the topic to join, broadcast messages for signaling, and establish direct QUIC connections for media and file transfer.

### Channel Identity

A channel is identified by a **channel secret** — a random 32-byte value generated at creation. The gossip `TopicId` is derived deterministically:

```rust
fn topic_from_secret(secret: &[u8; 32]) -> TopicId {
    let hash = Sha256::new()
        .chain_update(b"bisc-channel-v1:")
        .chain_update(secret)
        .finalize();
    TopicId::from_bytes(hash.into())
}
```

Only peers who know the secret can derive the `TopicId` and join the channel.

### The Channel Ticket

To join a channel, a peer needs a **BiscTicket** — a compact token shared out-of-band (clipboard, QR code, chat message):

```rust
struct BiscTicket {
    channel_secret: [u8; 32],
    bootstrap_addrs: Vec<EndpointAddr>,  // one or more peers to connect through
}
```

Serialized as a base32 string: `bisc1aefoq3k7...` (~100-200 characters).

### Flow: Create a Channel

```
1. Peer A generates channel_secret = random 32 bytes
2. Peer A derives topic_id from the secret
3. Peer A subscribes to the gossip topic (no bootstrap peers — A is first)
4. Peer A generates BiscTicket { secret, bootstrap: [A's address] }
5. Peer A displays ticket in UI (copy button, QR code)
6. Peer A broadcasts PeerAnnounce on the topic
```

### Flow: Join a Channel

```
1. Peer B receives ticket (paste / QR scan)
2. Peer B derives the same topic_id from the ticket's secret
3. Peer B subscribes to the gossip topic, bootstrapping via the ticket's addresses
4. Peer B broadcasts PeerAnnounce on the topic
5. Existing peers receive B's announcement
6. Each peer establishes a direct QUIC connection to B
7. Media negotiation happens over a reliable stream (MediaOffer/MediaAnswer)
8. Media flows as QUIC datagrams
```

### Flow: N-th Peer Joins

The same ticket works for any number of peers. The joining peer bootstraps via the addresses in the ticket, and iroh-gossip's HyParView protocol handles discovery of all existing members. The joiner doesn't need to know about every peer — gossip propagates the announcement.

### The "Creator Leaves" Problem — Solved

There is no "creator" in the running system. The creator's only special role is providing the initial bootstrap address in the ticket. Once the gossip swarm forms, all peers are equal.

When the creator leaves, remaining peers are unaffected. Any peer can generate a **refreshed ticket** with the same `channel_secret` but their own address as bootstrap:

```rust
fn refresh_ticket(&self) -> BiscTicket {
    BiscTicket {
        channel_secret: self.channel_secret,  // same secret = same topic
        bootstrap_addrs: vec![self.endpoint.addr()],
    }
}
```

For extra resilience, tickets can include multiple bootstrap addresses — the joining peer tries them in parallel.

### No Server Required

| Infrastructure | Who operates it | Purpose | Required? |
|---|---|---|---|
| iroh relay servers (DERP) | n0-computer (public) | NAT traversal coordination, relay fallback | Yes |
| Mainline BitTorrent DHT | Public infrastructure | Persistent channel discovery (optional) | No |

The relay servers are public infrastructure (like public STUN servers in WebRTC). They assist with NAT traversal and relay packets when hole-punching fails. They are **not** application servers — they don't know about channels, users, or media.

---

## Gossip Protocol Messages

All channel coordination happens via gossip broadcasts, serialized with `postcard` (compact binary):

```rust
enum ChannelMessage {
    PeerAnnounce {
        endpoint_id: EndpointId,
        display_name: String,
        capabilities: MediaCapabilities,
    },
    PeerLeave {
        endpoint_id: EndpointId,
    },
    Heartbeat {
        endpoint_id: EndpointId,
        timestamp: u64,
    },
    MediaStateUpdate {
        endpoint_id: EndpointId,
        audio_muted: bool,
        video_enabled: bool,
        screen_sharing: bool,
        app_audio_sharing: bool,
    },
    FileAnnounce {
        endpoint_id: EndpointId,
        file_hash: [u8; 32],
        file_name: String,
        file_size: u64,
        chunk_count: u32,
    },
    TicketRefresh {
        new_bootstrap: EndpointAddr,
    },
}
```

---

## Media Transport Over QUIC Datagrams

### Why Not WebRTC

WebRTC (via `str0m`) requires a signaling server for SDP/ICE exchange — the exact thing we want to eliminate. iroh already provides everything WebRTC does for a native-only app: NAT traversal, hole punching, encrypted transport, reliable streams, and unreliable datagrams.

What we lose: str0m's built-in jitter buffer, bandwidth estimation, and RTP packetization. What we gain: no signaling server, single transport, simpler architecture, faster connection setup.

### Media Negotiation (Replaces SDP)

Peers negotiate codecs via a simple exchange over a reliable QUIC stream:

```rust
struct MediaOffer {
    audio_codecs: Vec<CodecInfo>,    // e.g. Opus 48kHz
    video_codecs: Vec<CodecInfo>,    // e.g. H264, VP8
    max_resolution: (u32, u32),
    max_framerate: u32,
}

struct MediaAnswer {
    selected_audio_codec: CodecInfo,
    selected_video_codec: CodecInfo,
    negotiated_resolution: (u32, u32),
    negotiated_framerate: u32,
}
```

### Frame Packetization

Audio and video frames are encoded and sent as QUIC datagrams. Large frames are fragmented to fit within the datagram size limit (~1200 bytes):

```rust
struct MediaPacket {
    stream_id: u8,         // 0 = audio, 1 = camera, 2 = screen, etc.
    sequence: u32,         // monotonic sequence number
    timestamp: u32,        // media timestamp (90kHz video, 48kHz audio)
    fragment_index: u8,    // fragment N of M
    fragment_count: u8,    // total fragments for this frame
    is_keyframe: bool,
    payload: Bytes,        // encoded media data
}
```

### Media Engine (Application Layer)

Since we're not using WebRTC's media stack, we build a lightweight equivalent:

| Component | Purpose | Complexity |
|---|---|---|
| Jitter buffer | Reorder packets, smooth playback timing | Medium |
| Frame reassembly | Combine fragments into complete frames | Low |
| Keyframe requests | Request IDR when decoder loses sync | Low |
| Congestion estimation | Measure RTT and loss rate, adjust quality | Medium |
| Adaptive bitrate | Reduce encoder quality when congested | Medium |

Control messages flow over a reliable stream per peer connection:

```rust
enum MediaControl {
    RequestKeyframe { stream_id: u8 },
    ReceiverReport {
        stream_id: u8,
        packets_received: u32,
        packets_lost: u32,
        jitter: u32,
        rtt_estimate_ms: u16,
    },
    QualityChange {
        stream_id: u8,
        bitrate_bps: u32,
        resolution: (u32, u32),
        framerate: u32,
    },
}
```

---

## Video Frame Pipeline (Zero-Copy GPU Path)

```
Capture (camera/screen via scap/nokhwa)
  -> Encode (openh264/vpx)
  -> Packetize (MediaPacket fragments)
  -> Send (QUIC datagrams via iroh)
  --- network ---
  -> Receive (QUIC datagrams)
  -> Jitter buffer + reassemble
  -> Decode (openh264/vpx)
  -> wgpu texture upload (single memcpy, no IPC)
  -> Iced shader widget renders as GPU quad
```

Each video stream is rendered by a custom Iced `Shader` widget backed by a wgpu texture. Multiple streams are arranged in a responsive grid. Frame updates trigger `queue.write_texture()` and a view redraw.

---

## Library Selection

### Networking

| Crate | Purpose |
|---|---|
| `iroh` | P2P QUIC connections, NAT traversal, hole punching, relay fallback |
| `iroh-gossip` | Pub/sub channel membership, signaling, presence |

### Audio

| Crate | Purpose |
|---|---|
| `cpal` | Cross-platform microphone input and speaker output |
| `opus` | Real-time audio encoding/decoding |
| `webrtc-audio-processing` | Echo cancellation, noise suppression, auto-gain control |

### Video

| Crate | Purpose |
|---|---|
| `openh264` | H.264 encoding/decoding (Cisco's BSD-licensed codec) |
| `vpx-sys` | VP8/VP9 encoding/decoding |
| `nokhwa` | Camera capture (cross-platform) |

### Screen & Window Capture

**`scap`** — Cross-platform screen capture (DXGI/WGC on Windows, ScreenCaptureKit on macOS, PipeWire/X11 on Linux). Supports specific windows or full displays.

### Storage & Settings

| Crate | Purpose |
|---|---|
| `rusqlite` (bundled) | Local SQLite for file metadata, chunk status, peer catalog |
| `directories` | Cross-platform standard paths |
| `serde` + `toml` | User settings as TOML config file |

---

## Platform-Specific: Scoped Audio Capture

Each platform has a different API for per-application audio capture, unified behind a common trait:

```rust
trait AppAudioCapture {
    fn list_capturable_apps(&self) -> Vec<AppAudioSource>;
    fn start_capture(&mut self, source: &AppAudioSource) -> AudioStream;
    fn stop_capture(&mut self);
}
```

### Windows (10 2004+)

`wasapi` crate — `AudioClient::new_application_loopback_client(pid, include_tree)` captures audio from a specific process. Enumerate audio sessions via `IAudioSessionEnumerator`.

### macOS (13+)

ScreenCaptureKit via `objc2` bindings — `SCContentFilter` targets a specific `SCRunningApplication` with `capturesAudio = true`.

### Linux

PipeWire via `pipewire-rs` — create a virtual sink and link a specific application's audio node. Fallback to PulseAudio `pa_stream_set_monitor_stream` on older systems. Both loaded via `dlopen` for graceful degradation.

---

## File Sharing Protocol

Built on iroh reliable QUIC streams between peers. File announcements go via gossip so all channel members see shared files.

1. **SHARE** — Sender broadcasts `FileAnnounce` on the gossip topic (name, size, SHA-256 hash, chunk count).
2. **REQUEST** — Recipient clicks "Download", opens a reliable stream to any peer who has the file.
3. **CHUNK TRANSFER** — Files split into 256 KB chunks, downloadable from multiple peers simultaneously. Peers track chunks via bitfields.
4. **VERIFY** — Completed file verified against SHA-256, assembled in storage directory.
5. **AVAILABILITY** — Peers announce which files they have. New joiners receive the file catalog. Enables peer-assisted downloads when the original sender is offline.

### Storage Layout

```
<user-selected-dir>/bisc/<file_hash>/<filename>
```

---

## Application State (Elm Architecture)

```rust
struct Bisc {
    // Networking
    endpoint: iroh::Endpoint,
    gossip: iroh_gossip::Gossip,

    // Channel
    channel: Option<Channel>,

    // Media
    media_engine: MediaEngine,
    local_camera: Option<CameraStream>,
    local_mic: Option<AudioStream>,
    remote_video_surfaces: HashMap<(EndpointId, u8), VideoSurface>,

    // Screen/App sharing
    screen_shares: Vec<ScreenShare>,
    audio_shares: Vec<AudioShare>,

    // File sharing
    shared_files: Vec<SharedFile>,
    transfers: Vec<Transfer>,
    storage_dir: PathBuf,

    // Settings
    video_quality: QualityPreset,
    audio_quality: QualityPreset,
}

struct Channel {
    secret: [u8; 32],
    topic_id: TopicId,
    gossip_sender: iroh_gossip::GossipSender,
    peers: HashMap<EndpointId, PeerConnection>,
    display_name: String,
}

struct PeerConnection {
    connection: iroh::endpoint::Connection,
    control_stream: (SendStream, RecvStream),
    remote_state: MediaState,
    jitter_buffers: HashMap<u8, JitterBuffer>,
    display_name: String,
}

enum Message {
    // Channel
    CreateChannel,
    JoinChannel(String),           // ticket string
    LeaveChannel,
    TicketGenerated(String),

    // Gossip events
    PeerAnnounced(EndpointId, String, MediaCapabilities),
    PeerLeft(EndpointId),
    PeerConnected(EndpointId),
    PeerDisconnected(EndpointId),

    // Media
    ToggleCamera,
    ToggleMic,
    VideoFrameReceived(EndpointId, u8, VideoFrame),
    AudioSamplesReceived(EndpointId, Vec<f32>),
    KeyframeRequested(EndpointId, u8),

    // Sharing
    StartScreenShare(DisplayId),
    StartAppShare(WindowId),
    StartAudioShare(AppAudioSource),
    StopShare(ShareId),

    // Files
    ShareFile(PathBuf),
    RequestDownload(FileHash),
    ChunkReceived(FileHash, u32, Vec<u8>),
    TransferComplete(FileHash),

    // Settings
    SetVideoQuality(QualityPreset),
    SetAudioQuality(QualityPreset),
    SetStorageDir(PathBuf),
}
```

---

## Scaling Considerations

Full mesh topology works well for small groups but degrades beyond ~6 peers (each peer sends N-1 copies of their media).

**For larger channels:**
- Designate one well-connected peer as a lightweight SFU — it receives all streams and forwards them. Rotate the role based on bandwidth. No extra server needed.
- Audio-only by default for large groups, with video on demand (click to view a peer's video).

---

## Packaging & Distribution

Single binary with no runtime dependencies (beyond GPU drivers):

| Platform | Format | Details |
|---|---|---|
| Windows | Portable `.exe` | No installer required. Statically link MSVC runtime (`-C target-feature=+crt-static`). |
| macOS | `.app` in `.dmg` | Code signing and notarization via `apple-codesign` or Xcode CLI. |
| Linux | Static binary or AppImage | `x86_64-unknown-linux-musl` for fully static. AppImage for desktop integration. |

### Static Linking Strategy

- `rustls` instead of OpenSSL (pure Rust TLS).
- `rusqlite` with `bundled` feature (compiles SQLite into binary).
- `opus` with `static` feature.
- PipeWire/PulseAudio loaded via `dlopen` at runtime (graceful degradation).
- wgpu uses system GPU drivers (Vulkan/Metal/DX12/OpenGL fallback).

**Estimated binary size**: 15-30 MB.

---

## Risks & Mitigations

| Risk | Mitigation |
|---|---|
| Iced is pre-1.0 (0.14) | Last planned pre-1.0 release. COSMIC desktop depends on it at scale. Pin to stable releases. |
| Custom media engine (jitter buffer, congestion) is complex | Start with a simple jitter buffer; iterate. Audio-only calls are simpler to get right first. Congestion estimation can start with basic loss-rate measurement. |
| QUIC datagram size limits require frame fragmentation | Straightforward implementation — fragment on send, reassemble on receive. Well-understood problem. |
| Full mesh doesn't scale past ~6 peers | Peer-as-SFU for larger groups. Audio-only fallback. Sufficient for the target use case. |
| Per-app audio on Linux requires PipeWire | PipeWire is default on modern distros. PulseAudio fallback. Dynamic loading via `dlopen`. |
| Building polished UI without CSS is slower | Invest in reusable widget library early. COSMIC's `libcosmic` demonstrates the pattern. |

---

## Development Phases

### Phase 1 — Foundation & Channel Infrastructure (Weeks 1-4)
- Cargo workspace (`bisc-ui`, `bisc-media`, `bisc-net`, `bisc-files`)
- Iced application scaffold with Elm architecture
- Reusable widget library (buttons, panels, inputs, modals)
- iroh Endpoint setup
- iroh-gossip integration
- BiscTicket creation, parsing, display (copy/QR)
- Channel create/join/leave flow
- Gossip message protocol

### Phase 2 — Voice Calls (Weeks 5-8)
- Audio capture (`cpal`) and Opus encoding/decoding
- MediaPacket framing over QUIC datagrams
- Audio jitter buffer
- MediaOffer/MediaAnswer negotiation
- Basic congestion detection via ReceiverReports
- Call UI (join/leave, mute, peer list)

### Phase 3 — Video Calls (Weeks 9-14)
- Camera capture (`nokhwa`) and H264/VP8 encoding
- Frame fragmentation and reassembly
- Video jitter buffer
- Keyframe request mechanism
- Adaptive bitrate based on congestion feedback
- `VideoSurface` custom shader widget (wgpu)
- Multi-stream video grid layout

### Phase 4 — Screen & App Sharing (Weeks 15-20)
- Full screen capture via `scap`
- Window-specific capture
- Per-application scoped audio (platform-specific `AppAudioCapture` implementations)
- Source selection UI
- Multi-viewer support

### Phase 5 — File Sharing (Weeks 21-26)
- File chunking and transfer over iroh reliable streams
- `FileAnnounce` via gossip
- Peer-assisted downloads (bitfield-based chunk availability)
- Storage directory configuration (first-run + settings)
- File management UI

### Phase 6 — Polish & Ship (Weeks 27-30)
- Static linking and binary size optimization
- Packaging (portable .exe, .app/.dmg, static Linux binary)
- Code signing and notarization (macOS)
- Theming (light/dark mode)
- Performance profiling
- Multi-bootstrap tickets, ticket refresh, heartbeat-based timeout detection
