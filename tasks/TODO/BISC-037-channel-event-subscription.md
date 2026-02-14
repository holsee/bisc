# BISC-037: Channel Event Subscription — Networking Events to UI

**Phase**: 8 — Integration
**Depends on**: BISC-036

## Problem Statement

`Channel` emits `ChannelEvent` values (PeerJoined, PeerLeft, PeerConnected, MediaStateChanged, FileAnnounced) via an `mpsc` receiver, but nothing reads them. The UI has no awareness of peer presence, media state changes, or file announcements. The Iced subscription system needs to bridge these async events into `AppMessage` values.

## Deliverables

### 1. Channel Event Subscription

Create an `iced::Subscription` that reads from `Channel::recv_event()` and maps events to `AppMessage`:

| ChannelEvent | AppMessage |
|---|---|
| `PeerJoined(info)` | Add peer to `CallScreen` peer list |
| `PeerConnected(id)` | Mark peer as connected (ready for media) |
| `PeerLeft(id)` | Remove peer from `CallScreen` |
| `MediaStateChanged { .. }` | Update peer's media indicators (muted, video, sharing) |
| `FileAnnounced { .. }` | Add file to `FilesPanel` file list |

### 2. Peer List in CallScreen

Wire the peer list so it reflects live channel state:

- `CallScreen` shows the display name and media state of each connected peer
- Peers appear when `PeerJoined` fires, disappear on `PeerLeft`
- Media state icons update on `MediaStateChanged`

### 3. File Announcements in FilesPanel

- When `FileAnnounced` arrives, add the file to `FilesPanel`'s shared files list
- Show file name, size, and sender name
- Download button per file (triggers `AppAction::DownloadFile`)

### 4. Connection Status Indicator

- Show connection state in the UI (connecting, connected, disconnected)
- If `Channel::join()` is in progress, show a spinner or "Connecting..." state
- If gossip subscription drops, show reconnection state

## Acceptance Criteria

- [ ] Creating a channel and having a second peer join shows the peer in the call screen
- [ ] Peer leaving removes them from the peer list
- [ ] File announcements appear in the files panel
- [ ] Media state changes (mute, camera, screen share) update peer indicators
- [ ] No event is silently dropped — every `ChannelEvent` maps to a UI update
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo fmt --all --check` passes
