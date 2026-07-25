//! SSE (Server-Sent Events) transport internals.
//!
//! This module provides the low-level SSE machinery: connection state,
//! the background connection task, and the SSE stream type.
//!
//! For the high-level entry point, see [`page::live_view_page`].
//!
//! [`page::live_view_page`]: crate::page::live_view_page

use crate::{
    event_data::EventData,
    life_cycle::{
        spawn_view, EventMessageFromSocketData, UpdateResponse, ViewRequestError, ViewTaskHandle,
    },
    live_view::ViewHandle,
    util::ReceiverStream,
    LiveView,
};
use axum::response::sse::Event;
use futures_util::stream::Stream;
use http::{HeaderMap, Uri};
use pin_project_lite::pin_project;
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    convert::Infallible,
    fmt,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::{broadcast, mpsc, RwLock};

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Shared state for managing SSE connections.
///
/// Links incoming POST events to active SSE streams via unique connection IDs.
#[derive(Clone)]
pub struct LiveViewSseState {
    inner: Arc<RwLock<HashMap<ConnectionId, ConnectionHandle>>>,
}

impl fmt::Debug for LiveViewSseState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveViewSseState").finish()
    }
}

/// Unique identifier for an active SSE connection.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub(crate) String);

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl LiveViewSseState {
    /// Create a new, empty SSE connection state.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub(crate) async fn insert(&self, id: ConnectionId, handle: ConnectionHandle) {
        self.inner.write().await.insert(id, handle);
    }

    pub(crate) async fn remove(&self, id: &ConnectionId) -> Option<ConnectionHandle> {
        self.inner.write().await.remove(id)
    }

    pub(crate) async fn get(&self, id: &ConnectionId) -> Option<ConnectionHandle> {
        self.inner.read().await.get(id).cloned()
    }
}

impl Default for LiveViewSseState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Connection handle
// ---------------------------------------------------------------------------

/// A handle to an active SSE connection.
#[derive(Clone, Debug)]
pub(crate) struct ConnectionHandle {
    pub(crate) event_tx: mpsc::Sender<RawSseEvent>,
    pub(crate) update_tx: broadcast::Sender<SseServerMessage>,
}

/// A raw client event, deserialized from the POST request body.
#[derive(Debug, Clone)]
pub(crate) struct RawSseEvent {
    pub(crate) msg: String,
    pub(crate) event_type: String,
    pub(crate) data: Option<Value>,
}

/// Server→client messages sent over the SSE stream.
#[derive(Debug, Clone)]
pub(crate) enum SseServerMessage {
    InitialRender { id: String, html: Value },
    Render(Value),
    JsCommands(Vec<crate::js_command::JsCommand>),
    Health,
}

impl Serialize for SseServerMessage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            SseServerMessage::InitialRender { id, html } => {
                let mut s = serializer.serialize_map(Some(3))?;
                s.serialize_entry("t", "i")?;
                s.serialize_entry("id", id)?;
                s.serialize_entry("d", html)?;
                s.end()
            }
            SseServerMessage::Render(d) => {
                let mut s = serializer.serialize_map(Some(2))?;
                s.serialize_entry("t", "r")?;
                s.serialize_entry("d", d)?;
                s.end()
            }
            SseServerMessage::JsCommands(cmds) => {
                let mut s = serializer.serialize_map(Some(2))?;
                s.serialize_entry("t", "j")?;
                s.serialize_entry("d", cmds)?;
                s.end()
            }
            SseServerMessage::Health => {
                let mut s = serializer.serialize_map(Some(1))?;
                s.serialize_entry("t", "h")?;
                s.end()
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Connection ID generation
// ---------------------------------------------------------------------------

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
// Broadcast → mpsc bridge
// ---------------------------------------------------------------------------

pub(crate) fn broadcast_to_mpsc(
    mut broadcast_rx: broadcast::Receiver<SseServerMessage>,
) -> mpsc::Receiver<SseServerMessage> {
    let (tx, rx) = mpsc::channel(64);
    tokio::spawn(async move {
        loop {
            match broadcast_rx.recv().await {
                Ok(msg) => {
                    if tx.send(msg).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(amt)) => {
                    tracing::warn!(lagged = amt, "SSE broadcast receiver lagged");
                    let _ = tx.send(SseServerMessage::Health).await;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    rx
}

// ---------------------------------------------------------------------------
// SSE connection task
// ---------------------------------------------------------------------------

/// Spawn a background task that owns the view and processes incoming events
/// from the SSE stream's POST counterpart.
pub(crate) async fn run_sse_connection<L>(
    view: L,
    uri: Uri,
    headers: HeaderMap,
    mut event_rx: mpsc::Receiver<RawSseEvent>,
    update_tx: broadcast::Sender<SseServerMessage>,
    connection_id: ConnectionId,
    state: LiveViewSseState,
) where
    L: LiveView,
{
    let (view_handle, mut view_handle_rx) = ViewHandle::new();
    let view_task = spawn_view(view, Some(view_handle.clone()));

    if let Err(err) = view_task.mount(uri, headers, view_handle).await {
        tracing::error!(%err, "failed to mount SSE view");
        state.remove(&connection_id).await;
        return;
    }

    // Send initial render with connection ID
    match view_task.render().await {
        Ok(markup) => {
            let _ = update_tx.send(SseServerMessage::InitialRender {
                id: connection_id.0.clone(),
                html: markup,
            });
        }
        Err(err) => {
            tracing::error!(%err, "failed to render SSE view");
            state.remove(&connection_id).await;
            return;
        }
    }

    loop {
        if state.get(&connection_id).await.is_none() {
            break;
        }

        tokio::select! {
            maybe_event = event_rx.recv() => {
                match maybe_event {
                    Some(raw) => {
                        match process_raw_event::<L::Message>(&view_task, raw).await {
                            Ok(response) => broadcast_update(&update_tx, response),
                            Err(err) => {
                                tracing::error!(%err, "error processing SSE event (continuing)");
                            }
                        }
                    }
                    None => break,
                }
            }
            maybe_msg = view_handle_rx.recv() => {
                match maybe_msg {
                    Some(msg) => {
                        match view_task.update(msg, None).await {
                            Ok(response) => broadcast_update(&update_tx, response),
                            Err(err) => {
                                tracing::error!(%err, "error processing ViewHandle message");
                                break;
                            }
                        }
                    }
                    None => break,
                }
            }
        }
    }

    state.remove(&connection_id).await;
}

async fn process_raw_event<M>(
    view_task: &ViewTaskHandle<M>,
    raw: RawSseEvent,
) -> Result<UpdateResponse, ViewRequestError>
where
    M: serde::de::DeserializeOwned + PartialEq + Send + Sync + 'static,
{
    // Heartbeat or empty message — nothing to process
    if raw.msg.is_empty() && raw.event_type == "h" {
        return Ok(UpdateResponse::Empty);
    }

    if raw.msg.is_empty() {
        tracing::warn!(
            event_type = %raw.event_type,
            "SSE event with empty message"
        );
        return Ok(UpdateResponse::Empty);
    }

    let decoded = percent_encoding::percent_decode_str(&raw.msg)
        .decode_utf8()
        .map_err(|e| {
            tracing::error!(%e, "failed to decode percent-encoded message");
            ViewRequestError::ChannelClosed(crate::life_cycle::ChannelClosed)
        })?;

    let msg: M = serde_json::from_str(&decoded).map_err(|e| {
        tracing::error!(%e, "failed to deserialize message");
        ViewRequestError::ChannelClosed(crate::life_cycle::ChannelClosed)
    })?;

    let event_data: Option<EventData> = if raw.event_type.is_empty() {
        None
    } else if let Some(data_value) = &raw.data {
        match serde_json::from_value::<EventMessageFromSocketData>(serde_json::json!({
            "t": &raw.event_type,
            "d": data_value,
        })) {
            Ok(parsed) => Option::<EventData>::from(parsed),
            Err(e) => {
                tracing::error!(%e, "failed to parse event data");
                None
            }
        }
    } else {
        match serde_json::from_value::<EventMessageFromSocketData>(serde_json::json!({
            "t": &raw.event_type,
        })) {
            Ok(parsed) => Option::<EventData>::from(parsed),
            Err(_) => None,
        }
    };

    view_task.update(msg, event_data).await
}

fn broadcast_update(update_tx: &broadcast::Sender<SseServerMessage>, response: UpdateResponse) {
    match response {
        UpdateResponse::Diff(diff) => {
            let _ = update_tx.send(SseServerMessage::Render(diff));
        }
        UpdateResponse::JsCommands(commands) => {
            let _ = update_tx.send(SseServerMessage::JsCommands(commands));
        }
        UpdateResponse::DiffAndJsCommands(diff, commands) => {
            let _ = update_tx.send(SseServerMessage::Render(diff));
            let _ = update_tx.send(SseServerMessage::JsCommands(commands));
        }
        UpdateResponse::Empty => {}
    }
}

// ---------------------------------------------------------------------------
// SSE stream type
// ---------------------------------------------------------------------------

pin_project! {
    pub(crate) struct SseStream {
        #[pin]
        pub(crate) rx: ReceiverStream<SseServerMessage>,
        pub(crate) state: LiveViewSseState,
        pub(crate) connection_id: ConnectionId,
    }

    impl PinnedDrop for SseStream {
        fn drop(this: Pin<&mut Self>) {
            let state = this.state.clone();
            let connection_id = this.connection_id.clone();
            tokio::spawn(async move {
                state.remove(&connection_id).await;
            });
        }
    }
}

impl Stream for SseStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        match this.rx.poll_next(cx) {
            Poll::Ready(Some(msg)) => {
                let data = serde_json::to_string(&msg).unwrap_or_default();
                Poll::Ready(Some(Ok(Event::default().data(data))))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}
