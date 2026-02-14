# BISC-036: Iced Application Entrypoint & Async Bridge

**Phase**: 8 — Integration
**Depends on**: BISC-030

## Problem Statement

`main.rs` logs "bisc starting" and exits. The Iced `Application` trait is never implemented, no window opens, no tokio runtime is bridged to the UI event loop. The `App` state machine produces `AppAction` values that nothing consumes. This is the foundation that everything else blocks on.

## Deliverables

### 1. Iced Application Implementation

Wire `bisc-app/src/main.rs` to actually run an Iced application:

- Implement `iced::Application` (or use `iced::application()` builder) for `App`
- `view()` delegates to the active screen's `view()` based on `self.screen`
- `update()` calls `App::update()` and dispatches the returned `AppAction`
- `theme()` returns a default theme
- Window title: "bisc"

### 2. Tokio Runtime Bridge

Iced runs its own event loop. Networking (iroh, gossip) needs tokio. Bridge them:

- Create a shared `tokio::runtime::Runtime` (or use `iced::subscription` with async)
- Hold networking state (`BiscEndpoint`, `GossipHandle`, `Channel`) in the app or a side struct accessible from `update()`
- Use `iced::Task` (command pattern) to spawn async operations from `update()` and return results as messages
- Alternatively, use `iced::Subscription` for long-lived async event streams

### 3. AppAction Dispatch

Each `AppAction` variant must trigger real work:

| AppAction | Dispatch |
|---|---|
| `CreateChannel(name)` | `Channel::create()` via async task → `AppMessage::ChannelCreated(ticket)` |
| `JoinChannel { ticket, name }` | Parse ticket, `Channel::join()` → `AppMessage::ChannelJoined` or `AppMessage::ChannelError` |
| `LeaveChannel` | `Channel::leave()`, tear down pipelines |
| `CopyToClipboard(text)` | `iced::clipboard::write()` |
| `SetMic(on)` | Toggle `VoicePipeline::set_muted()` |
| `SetCamera(on)` | Start/stop `VideoPipeline` |
| `SetScreenShare(on)` | Start/stop screen capture pipeline |
| `OpenFilePicker` | Native file dialog (or iced file picker) |
| `DownloadFile(hash)` | `FileTransfer::request()` via async task |
| `SaveSettings` | Write settings to disk |

### 4. Networking Lifecycle

- On app start: create `BiscEndpoint`, create `GossipHandle`
- On channel create/join: store the `Channel`, start accepting connections
- On channel leave: `Channel::leave()`, clear peer state
- On app exit: graceful shutdown of endpoint

## Acceptance Criteria

- [x] Running `cargo run` opens an Iced window showing the channel screen
- [x] Creating a channel produces a ticket string displayed in the UI
- [x] The app compiles and all existing tests still pass
- [x] `AppAction::CopyToClipboard` writes to the system clipboard
- [x] `AppAction::SaveSettings` persists settings to disk
- [x] Graceful shutdown: closing the window drops the endpoint cleanly
- [x] `cargo clippy --workspace -- -D warnings` passes
- [x] `cargo fmt --all --check` passes
