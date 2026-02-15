# BISC-051: End-to-End Manual Validation

**Phase**: 9 — Validation & Fixes
**Depends on**: BISC-049, BISC-050, BISC-057, BISC-058, BISC-059, BISC-060, BISC-061

## Problem Statement

With 48 tasks implemented by automated agents, the application has never been validated end-to-end from a real user's perspective. The README defines the user-facing requirements. We need to verify each core user flow works in practice with two instances on the same machine.

## Deliverables

### 1. Channel Creation & Joining

- Instance A: set display name, create a channel, receive a ticket
- Instance B: paste ticket, set display name, join the channel
- Both instances show each other as connected peers
- Ticket copy button works

### 2. Voice Call

- Both peers have mic enabled by default on join
- Mute/unmute toggles audio (verify via logs showing packet flow start/stop)
- Peer disconnect cleans up voice pipeline on the other side

### 3. Video Call

- Enable camera on one peer — other peer sees video frames (verify via logs)
- Disable camera — frames stop
- Both peers enable camera — bidirectional video

### 4. Screen Sharing

- Enable screen share on one peer
- Other peer receives screen share frames (verify via logs)
- Stop sharing cleans up

### 5. File Sharing

- Peer A shares a file via file picker
- Peer B sees the file announcement in the files panel
- Peer B downloads the file — completes successfully
- Downloaded file matches original (SHA-256 verified in logs)

### 6. Settings

- Navigate to settings screen, change display name
- Save settings, verify they persist across restart
- Back button returns to previous screen

### 7. Rejoin Flow

- Leave channel on both peers
- Create a new channel — everything works again (fresh networking stack)

## Acceptance Criteria

- [x] Channel create/join works between two instances
- [x] Peers see each other in the call screen
- [x] Voice pipeline starts (verified via `PeerConnected` and voice pipeline logs)
- [x] File share + download works end-to-end
- [x] Settings save and load correctly
- [x] Leave and rejoin works without crash
- [x] No panics or `ERROR` level log messages during normal operation
- [x] Application runs stably for at least 2 minutes in a channel
