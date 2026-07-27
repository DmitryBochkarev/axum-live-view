//! SSE (Server-Sent Events) transport internals.
//!
//! This module provides the SSE-specific machinery: the background
//! connection task, broadcast→mpsc bridge, SSE stream type, and the
//! public `setup` / `live_page` / `event_handler` API.
//!
//! Shared types live in [`crate::transport`].

use crate::{
    LiveView,
    life_cycle::spawn_view,
    live_view::ViewHandle,
    transport::{
        ConnectionId, ConnectionRegistry, RawEvent, ServerMessage, forward_to_broadcast,
        process_client_event,
    },
    util::ReceiverStream,
};
use axum::{
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response, sse::Event},
};
use futures_util::stream::Stream;
use pin_project_lite::pin_project;
use serde_json::Value;
use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::sync::{broadcast, mpsc};

// ---------------------------------------------------------------------------
// Backward-compatible type alias
// ---------------------------------------------------------------------------

/// Shared state for managing live-view connections (both SSE and long-poll).
///
/// This is a type alias for [`ConnectionRegistry`]. The name is kept for
/// backward compatibility.
///
/// [`ConnectionRegistry`]: crate::transport::ConnectionRegistry
pub type LiveViewSseState = ConnectionRegistry;

// ---------------------------------------------------------------------------
// Broadcast → mpsc bridge
// ---------------------------------------------------------------------------

pub(crate) fn broadcast_to_mpsc(
    mut broadcast_rx: broadcast::Receiver<ServerMessage>,
) -> mpsc::Receiver<ServerMessage> {
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
                    let _ = tx.send(ServerMessage::Health).await;
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
    mut event_rx: mpsc::Receiver<RawEvent>,
    update_tx: broadcast::Sender<ServerMessage>,
    connection_id: ConnectionId,
    state: ConnectionRegistry,
) where
    L: LiveView,
{
    let (view_handle, mut view_handle_rx) = ViewHandle::new();
    let view_task = spawn_view(view, Some(view_handle.clone()));

    if let Err(err) = view_task.mount(uri, headers, view_handle).await {
        tracing::error!(%err, "failed to mount SSE view");
        state.remove(&connection_id);
        return;
    }

    // Send initial render with connection ID
    match view_task.render().await {
        Ok(markup) => {
            let _ = update_tx.send(ServerMessage::InitialRender {
                id: connection_id.0.clone(),
                html: markup,
            });
        }
        Err(err) => {
            tracing::error!(%err, "failed to render SSE view");
            state.remove(&connection_id);
            return;
        }
    }

    loop {
        if state.get(&connection_id).is_none() {
            break;
        }

        tokio::select! {
            maybe_event = event_rx.recv() => {
                match maybe_event {
                    Some(raw) => {
                        match process_client_event::<L::Message>(&view_task, raw).await {
                            Ok(response) => forward_to_broadcast(&update_tx, response),
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
                            Ok(response) => forward_to_broadcast(&update_tx, response),
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

    state.remove(&connection_id);
}

// ---------------------------------------------------------------------------
// High-level setup
// ---------------------------------------------------------------------------

/// Enable live-view support on a router.
///
/// Wraps the given router, adding:
/// - A `/_live_view.js` GET route that serves the precompiled live-view JavaScript
/// - An `Extension` layer with [`LiveViewSseState`] so [`LiveViewUpgrade`] can
///   detect SSE requests and [`live_page`] can handle SSE events
/// - An `Extension` layer for long-poll connections
///
/// Use [`live_page`] instead of [`get`](axum::routing::get) for routes that
/// should support SSE/long-poll transport:
///
/// ```rust,ignore
/// use axum::Router;
/// use axum_live_view::{live_page, LiveViewUpgrade};
///
/// let app = axum_live_view::setup(
///     Router::new()
///         .route("/", live_page(root))
/// );
/// ```
///
/// [`LiveViewUpgrade`]: crate::LiveViewUpgrade
pub fn setup(router: axum::Router) -> axum::Router {
    use axum::Extension;
    use std::sync::Arc;

    let registry = Arc::new(ConnectionRegistry::new());
    let lp_connections = Arc::new(crate::long_poll::LongPollConnections::new());
    let router = router
        .layer(Extension(registry))
        .layer(Extension(lp_connections));

    router.route("/_live_view.js", crate::precompiled_js())
}

// ---------------------------------------------------------------------------
// live_page helper
// ---------------------------------------------------------------------------

/// A method router for live-view routes that support SSE and long-poll transport.
///
/// Equivalent to `axum::routing::get(handler).post(event_handler)` — it
/// handles:
/// - `GET` requests: initial HTML render, WebSocket upgrade, SSE stream,
///   or long-poll (via [`LiveViewUpgrade`])
/// - `POST` requests: client events forwarded to the view (SSE and long-poll)
///
/// Use this instead of [`get`](axum::routing::get) for any route that hosts
/// a live view and needs SSE/long-poll fallback support. Routes using only
/// WebSocket transport can continue to use [`get`](axum::routing::get).
///
/// # Example
///
/// ```rust,ignore
/// use axum::Router;
/// use axum_live_view::{live_page, LiveViewUpgrade};
///
/// async fn root(live: LiveViewUpgrade) -> impl axum::response::IntoResponse {
///     // ...
///     # axum::response::Response::new(axum::body::Body::empty())
/// }
///
/// let app = axum_live_view::setup(
///     Router::new()
///         .route("/", live_page(root))
/// );
/// ```
///
/// [`LiveViewUpgrade`]: crate::LiveViewUpgrade
pub fn live_page<H, T, S>(handler: H) -> axum::routing::MethodRouter<S>
where
    H: axum::handler::Handler<T, S>,
    T: 'static,
    S: Clone + Send + Sync + 'static,
{
    axum::routing::get(handler).post(event_handler)
}

// ---------------------------------------------------------------------------
// POST event handler
// ---------------------------------------------------------------------------

/// Axum handler for receiving client events via POST.
///
/// The JavaScript client sends POST requests to the same URL with
/// `x-live-view-id: <connection-id>` headers when WebSocket transport
/// is unavailable (SSE and long-poll both use this endpoint).
///
/// This handler is automatically combined with the view handler by
/// [`live_page`]. Use [`live_page`] instead of [`get`](axum::routing::get)
/// for routes that should support SSE/long-poll transport.
pub async fn event_handler(
    axum::Extension(registry): axum::Extension<Arc<ConnectionRegistry>>,
    headers: HeaderMap,
    body: bytes::Bytes,
) -> Response {
    let connection_id = match headers.get("x-live-view-id").and_then(|v| v.to_str().ok()) {
        Some(id) => ConnectionId(id.to_owned()),
        None => return (StatusCode::BAD_REQUEST, "missing x-live-view-id header").into_response(),
    };

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("invalid JSON: {e}")).into_response();
        }
    };

    let raw_event = RawEvent {
        msg: payload
            .get("m")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned(),
        event_type: payload
            .get("t")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned(),
        data: payload.get("d").cloned(),
    };

    let handle = match registry.get(&connection_id) {
        Some(h) => h,
        None => return (StatusCode::NOT_FOUND, "unknown connection id").into_response(),
    };

    match handle.event_tx.send(raw_event).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(_) => (StatusCode::GONE, "connection closed").into_response(),
    }
}

// ---------------------------------------------------------------------------
// SSE stream type
// ---------------------------------------------------------------------------

pin_project! {
    pub(crate) struct SseStream {
        #[pin]
        pub(crate) rx: ReceiverStream<ServerMessage>,
        pub(crate) state: ConnectionRegistry,
        pub(crate) connection_id: ConnectionId,
    }

    impl PinnedDrop for SseStream {
        fn drop(this: Pin<&mut Self>) {
            let state = this.state.clone();
            let connection_id = this.connection_id.clone();
            tokio::spawn(async move {
                state.remove(&connection_id);
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
