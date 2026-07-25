//! Long-polling transport — fallback when WebSocket and SSE are unavailable.
//!
//! Long-polling holds an HTTP GET open until a server→client message
//! arrives or a 30 s timeout expires. Messages are buffered between
//! polls so nothing is lost. Client→server messages use the same POST
//! mechanism as SSE (via [`ConnectionRegistry`]).
//!
//! ## Protocol
//!
//! 1. **Initial poll:** `GET /path` with `Accept: text/x-live-view-longpoll`
//!    → response: `[{"t":"i","id":"<conn_id>","d":<html>}]`
//! 2. **Subsequent polls:** `GET /path` with `Accept: text/x-live-view-longpoll`
//!    and `x-live-view-id: <conn_id>` → response: array of accumulated messages
//!    or `[{"t":"h"}]` on timeout.
//! 3. **Client→server:** `POST /path` with `x-live-view-id: <conn_id>` and
//!    JSON body (same as SSE).
//!
//! [`ConnectionRegistry`]: crate::transport::ConnectionRegistry

use crate::{
    life_cycle::spawn_view,
    live_view::ViewHandle,
    transport::{
        ConnectionHandle, ConnectionId, ConnectionRegistry, RawEvent, ServerMessage,
        forward_to_push, new_connection_id, process_client_event,
    },
    LiveView,
};
use axum::{
    http::{HeaderMap, Uri},
};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
};
use tokio::sync::{broadcast, mpsc, Notify};

// ---------------------------------------------------------------------------
// Per-connection state
// ---------------------------------------------------------------------------

/// Per-connection state for one long-poll session.
///
/// Messages from the view task accumulate in `pending` and the next poll
/// drains them. `notify` wakes a waiting poll when new data arrives.
#[derive(Debug, Clone)]
struct LongPollConn {
    pending: Arc<Mutex<VecDeque<ServerMessage>>>,
    notify: Arc<Notify>,
    update_tx: broadcast::Sender<ServerMessage>,
}

impl LongPollConn {
    fn new(update_tx: broadcast::Sender<ServerMessage>) -> Self {
        Self {
            pending: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(Notify::new()),
            update_tx,
        }
    }

    fn push(&self, msg: ServerMessage) {
        let _ = self.update_tx.send(msg.clone());
        self.pending.lock().unwrap().push_back(msg);
        self.notify.notify_one();
    }
}

// ---------------------------------------------------------------------------
// Shared long-poll state map
// ---------------------------------------------------------------------------

/// Holds active long-poll connections, keyed by [`ConnectionId`].
///
/// Separate from [`ConnectionRegistry`] so the poll handler can access
/// the pending-message buffer directly.
#[derive(Clone, Debug)]
pub(crate) struct LongPollConnections {
    inner: Arc<Mutex<HashMap<ConnectionId, LongPollConn>>>,
}

impl LongPollConnections {
    pub(crate) fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn insert(&self, id: ConnectionId, conn: LongPollConn) {
        self.inner.lock().unwrap().insert(id, conn);
    }

    fn remove(&self, id: &ConnectionId) {
        self.inner.lock().unwrap().remove(id);
    }

    pub(crate) fn contains(&self, id: &ConnectionId) -> bool {
        self.inner.lock().unwrap().contains_key(id)
    }

    /// Wait for messages on the given connection, with a timeout.
    /// Drains and returns all pending messages, or `None` if the
    /// connection no longer exists.
    pub(crate) async fn wait_for_messages(
        &self,
        id: &ConnectionId,
        timeout: std::time::Duration,
    ) -> Option<Vec<ServerMessage>> {
        // Phase 1: check if there are already pending messages
        let (_pending, notify) = {
            let guard = self.inner.lock().unwrap();
            let conn = guard.get(id)?;
            let mut q = conn.pending.lock().unwrap();
            if !q.is_empty() {
                let msgs: Vec<_> = q.drain(..).collect();
                return Some(msgs);
            }
            (conn.pending.clone(), conn.notify.clone())
        };

        // Phase 2: wait for new messages or timeout
        tokio::select! {
            _ = notify.notified() => {
                let guard = self.inner.lock().unwrap();
                let conn = guard.get(id)?;
                let mut q = conn.pending.lock().unwrap();
                Some(q.drain(..).collect())
            }
            _ = tokio::time::sleep(timeout) => {
                let guard = self.inner.lock().unwrap();
                let conn = guard.get(id)?;
                let mut q = conn.pending.lock().unwrap();
                Some(q.drain(..).collect())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub(crate) enum LongPollResponse {
    Messages(Vec<ServerMessage>),
}

// ---------------------------------------------------------------------------
// Start a new connection
// ---------------------------------------------------------------------------

/// Create a new long-poll connection, spawn the background view task, and
/// register the connection in both the connection registry (for POST events)
/// and the long-poll state (for poll GETs).
pub(crate) fn start_long_poll_connection<L>(
    view: L,
    uri: Uri,
    headers: HeaderMap,
    registry: &ConnectionRegistry,
    lp_connections: &LongPollConnections,
) -> ConnectionId
where
    L: LiveView,
{
    let conn_id = ConnectionId(new_connection_id());

    let (event_tx, event_rx) = mpsc::channel(256);
    let (update_tx, _) = broadcast::channel(64);

    let lp_conn = LongPollConn::new(update_tx.clone());

    // Register in connection registry for POST events
    registry.insert(
        conn_id.clone(),
        ConnectionHandle {
            event_tx,
            update_tx: update_tx.clone(),
        },
    );

    // Register in long-poll state for poll GETs
    lp_connections.insert(conn_id.clone(), lp_conn.clone());

    // Spawn background task
    let task_lp_conn = lp_conn;
    let task_conn_id = conn_id.clone();
    let task_registry = registry.clone();
    let task_lp_connections = lp_connections.clone();
    tokio::spawn(async move {
        run_long_poll_view_task(
            view,
            uri,
            headers,
            event_rx,
            task_lp_conn,
            task_conn_id,
            task_registry,
            task_lp_connections,
        )
        .await;
    });

    conn_id
}

#[allow(clippy::too_many_arguments)]
async fn run_long_poll_view_task<L>(
    view: L,
    uri: Uri,
    headers: HeaderMap,
    mut event_rx: mpsc::Receiver<RawEvent>,
    lp_conn: LongPollConn,
    connection_id: ConnectionId,
    registry: ConnectionRegistry,
    lp_connections: LongPollConnections,
) where
    L: LiveView,
{
    let (view_handle, mut view_handle_rx) = ViewHandle::new();
    let view_task = spawn_view(view, Some(view_handle.clone()));

    if let Err(err) = view_task.mount(uri, headers, view_handle).await {
        tracing::error!(%err, "failed to mount long-poll view");
        cleanup(&registry, &lp_connections, &connection_id);
        return;
    }

    match view_task.render().await {
        Ok(markup) => {
            lp_conn.push(ServerMessage::InitialRender {
                id: connection_id.0.clone(),
                html: markup,
            });
        }
        Err(err) => {
            tracing::error!(%err, "long-poll initial render failed");
            cleanup(&registry, &lp_connections, &connection_id);
            return;
        }
    }

    loop {
        if !lp_connections.contains(&connection_id) {
            break;
        }

        tokio::select! {
            maybe_event = event_rx.recv() => {
                match maybe_event {
                    Some(raw) => {
                        match process_client_event::<L::Message>(&view_task, raw).await {
                            Ok(response) => forward_to_push(
                                |msg| lp_conn.push(msg),
                                response,
                            ),
                            Err(err) => {
                                tracing::error!(%err, "long-poll event error");
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
                            Ok(response) => forward_to_push(
                                |msg| lp_conn.push(msg),
                                response,
                            ),
                            Err(err) => {
                                tracing::error!(%err, "long-poll handle msg error");
                                break;
                            }
                        }
                    }
                    None => break,
                }
            }
        }
    }

    cleanup(&registry, &lp_connections, &connection_id);
}

fn cleanup(
    registry: &ConnectionRegistry,
    lp_connections: &LongPollConnections,
    id: &ConnectionId,
) {
    registry.remove(id);
    lp_connections.remove(id);
}
