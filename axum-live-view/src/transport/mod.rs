//! Shared transport-layer types used by SSE, long-polling, and WebSocket.
//!
//! This module contains the common connection registry, wire-format message
//! types, event parsing, and helpers that every transport implementation
//! shares.

use crate::{
    LiveView,
    event_data::EventData,
    life_cycle::{EventMessageFromSocketData, UpdateResponse, ViewRequestError, ViewTaskHandle},
    live_view::ViewHandle,
};
use axum::http::{HeaderMap, Uri};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};
use tokio::sync::{broadcast, mpsc};

// ---------------------------------------------------------------------------
// Connection ID
// ---------------------------------------------------------------------------

/// Unique identifier for an active live-view connection.
///
/// Shared across all transport types (WS, SSE, long-poll).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ConnectionId(pub(crate) String);

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Generate a new unique connection ID.
pub(crate) fn new_connection_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let a = now as u64;
    let b = a.wrapping_mul(6364136223846793005);
    format!("{:016x}{:016x}", a, b)
}

// ---------------------------------------------------------------------------
// Connection registry
// ---------------------------------------------------------------------------

/// Shared registry of active connections, keyed by [`ConnectionId`].
///
/// Both SSE and long-poll transport share the same registry so that
/// client→server POST events can reach either transport type.
#[derive(Clone)]
pub struct ConnectionRegistry {
    inner: Arc<Mutex<HashMap<ConnectionId, ConnectionHandle>>>,
}

impl fmt::Debug for ConnectionRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionRegistry").finish()
    }
}

impl ConnectionRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn insert(&self, id: ConnectionId, handle: ConnectionHandle) {
        self.inner.lock().unwrap().insert(id, handle);
    }

    pub(crate) fn remove(&self, id: &ConnectionId) -> Option<ConnectionHandle> {
        self.inner.lock().unwrap().remove(id)
    }

    pub(crate) fn get(&self, id: &ConnectionId) -> Option<ConnectionHandle> {
        self.inner.lock().unwrap().get(id).cloned()
    }
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Connection handle
// ---------------------------------------------------------------------------

/// Handle for pushing client events into a live-view connection's
/// background task. The `event_tx` sender forwards `POST`-ed events
/// from the HTTP layer into the view task's event loop.
#[derive(Clone, Debug)]
pub(crate) struct ConnectionHandle {
    pub(crate) event_tx: mpsc::Sender<RawEvent>,
    #[allow(dead_code)]
    pub(crate) update_tx: broadcast::Sender<ServerMessage>,
}

// ---------------------------------------------------------------------------
// Raw client event
// ---------------------------------------------------------------------------

/// A raw client event, parsed from a POST request body before being
/// deserialized into the view's `Message` type.
#[derive(Debug, Clone)]
pub(crate) struct RawEvent {
    pub(crate) msg: String,
    pub(crate) event_type: String,
    pub(crate) data: Option<Value>,
}

// ---------------------------------------------------------------------------
// Server → client message
// ---------------------------------------------------------------------------

/// Messages sent from the server to the client over any transport.
///
/// The wire format is the same regardless of transport (WS, SSE, or
/// long-poll). Each variant serializes to a compact JSON object with
/// single-letter field names.
#[derive(Debug, Clone)]
pub(crate) enum ServerMessage {
    InitialRender { id: String, html: Value },
    Render(Value),
    JsCommands(Vec<crate::js_command::JsCommand>),
    Health,
}

impl Serialize for ServerMessage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            ServerMessage::InitialRender { id, html } => {
                let mut s = serializer.serialize_map(Some(3))?;
                s.serialize_entry("t", "i")?;
                s.serialize_entry("id", id)?;
                s.serialize_entry("d", html)?;
                s.end()
            }
            ServerMessage::Render(d) => {
                let mut s = serializer.serialize_map(Some(2))?;
                s.serialize_entry("t", "r")?;
                s.serialize_entry("d", d)?;
                s.end()
            }
            ServerMessage::JsCommands(cmds) => {
                let mut s = serializer.serialize_map(Some(2))?;
                s.serialize_entry("t", "j")?;
                s.serialize_entry("d", cmds)?;
                s.end()
            }
            ServerMessage::Health => {
                let mut s = serializer.serialize_map(Some(1))?;
                s.serialize_entry("t", "h")?;
                s.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Event processing (shared)
// ---------------------------------------------------------------------------

/// Parse a [`RawEvent`] into the view's `Message` type and
/// [`EventData`], then call `view_task.update()`.
pub(crate) async fn process_client_event<M>(
    view_task: &ViewTaskHandle<M>,
    raw: RawEvent,
) -> Result<UpdateResponse, ViewRequestError>
where
    M: serde::de::DeserializeOwned + PartialEq + Send + Sync + 'static,
{
    // Heartbeat — nothing to process
    if raw.msg.is_empty() {
        if raw.event_type != "h" {
            tracing::warn!(event_type = %raw.event_type, "client event with empty message");
        }
        return Ok(UpdateResponse::Empty);
    }

    let decoded = percent_encoding::percent_decode_str(&raw.msg)
        .decode_utf8()
        .map_err(|_| ViewRequestError::ChannelClosed(crate::life_cycle::ChannelClosed))?;

    let msg: M = serde_json::from_str(&decoded)
        .map_err(|_| ViewRequestError::ChannelClosed(crate::life_cycle::ChannelClosed))?;

    let event_data = parse_event_data(&raw);

    view_task.update(msg, event_data).await
}

/// Parse event-type and optional payload from a [`RawEvent`] into
/// [`EventData`].
pub(crate) fn parse_event_data(raw: &RawEvent) -> Option<EventData> {
    if raw.event_type.is_empty() {
        return None;
    }
    if let Some(data_value) = &raw.data {
        serde_json::from_value::<EventMessageFromSocketData>(serde_json::json!({
            "t": &raw.event_type,
            "d": data_value,
        }))
        .ok()
        .and_then(Option::<EventData>::from)
    } else {
        serde_json::from_value::<EventMessageFromSocketData>(serde_json::json!({
            "t": &raw.event_type,
        }))
        .ok()
        .and_then(Option::<EventData>::from)
    }
}

/// Forward an [`UpdateResponse`] into a broadcast channel.
///
/// Each variant of `UpdateResponse` is converted to the appropriate
/// [`ServerMessage`](s) and sent on `update_tx`.
pub(crate) fn forward_to_broadcast(
    update_tx: &broadcast::Sender<ServerMessage>,
    response: UpdateResponse,
) {
    match response {
        UpdateResponse::Diff(diff) => {
            let _ = update_tx.send(ServerMessage::Render(diff));
        }
        UpdateResponse::JsCommands(commands) => {
            let _ = update_tx.send(ServerMessage::JsCommands(commands));
        }
        UpdateResponse::DiffAndJsCommands(diff, commands) => {
            let _ = update_tx.send(ServerMessage::Render(diff));
            let _ = update_tx.send(ServerMessage::JsCommands(commands));
        }
        UpdateResponse::Empty => {}
    }
}

/// Push [`UpdateResponse`] into a callback that accepts individual
/// [`ServerMessage`] values (e.g. for long-poll's pending-message buffer).
pub(crate) fn forward_to_push(push: impl Fn(ServerMessage), response: UpdateResponse) {
    match response {
        UpdateResponse::Diff(diff) => push(ServerMessage::Render(diff)),
        UpdateResponse::JsCommands(commands) => push(ServerMessage::JsCommands(commands)),
        UpdateResponse::DiffAndJsCommands(diff, commands) => {
            push(ServerMessage::Render(diff));
            push(ServerMessage::JsCommands(commands));
        }
        UpdateResponse::Empty => {}
    }
}

// ---------------------------------------------------------------------------
// View mounting (shared)
// ---------------------------------------------------------------------------

/// Mount a live view and render its initial state, sending the result
/// as an `InitialRender` message. Returns `Err(())` if mounting or
/// rendering fails, in which case the caller should clean up.
#[allow(dead_code)]
pub(crate) async fn mount_and_render<L>(
    view_task: &ViewTaskHandle<L::Message>,
    view_handle: ViewHandle<L::Message>,
    uri: Uri,
    headers: HeaderMap,
    _connection_id: &ConnectionId,
) -> Result<Value, ()>
where
    L: LiveView,
{
    view_task
        .mount(uri, headers, view_handle)
        .await
        .map_err(|err| {
            tracing::error!(%err, "failed to mount view");
        })?;

    view_task.render().await.map_err(|err| {
        tracing::error!(%err, "failed to render view");
    })
}
