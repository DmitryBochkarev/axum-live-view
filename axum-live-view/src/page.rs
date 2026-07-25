//! Page-level live view handler — the primary entry point for building live-view pages.
//!
//! [`live_view_page`] returns an axum [`Router`] that handles all transport modes
//! (HTML, WebSocket, SSE events) on a single route.
//!
//! SSE support requires a [`LiveViewSseState`] added via the [`Extension`] layer:
//!
//! ```rust,ignore
//! use std::sync::Arc;
//! use axum::{Router, Extension};
//! use axum_live_view::{html, page, sse::LiveViewSseState};
//!
//! let sse = Arc::new(LiveViewSseState::new());
//!
//! let app = Router::new()
//!     .merge(page::live_view_page("/", |embed| html! {
//!         <!DOCTYPE html><html><body>
//!             { embed.embed(MyView::default()) }
//!             <script src="/bundle.js"></script>
//!         </body></html>
//!     }))
//!     .route("/bundle.js", axum_live_view::precompiled_js())
//!     .layer(Extension(sse));
//! ```

use crate::{
    extract::run_view_on_socket,
    life_cycle::EmbedLiveView,
    sse::{
        broadcast_to_mpsc, new_connection_id, run_sse_connection, ConnectionHandle, ConnectionId,
        LiveViewSseState, RawSseEvent, SseServerMessage, SseStream,
    },
    util::ReceiverStream,
    LiveView,
};
use axum::{
    body::{self, Body},
    extract::{ws::WebSocketUpgrade, Extension, FromRequest},
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{
        sse::{KeepAlive, Sse},
        IntoResponse, Response,
    },
};
use std::sync::Arc;

/// Create a router that handles all live-view transport modes on a single route.
///
/// The returned [`axum::Router`] handles:
/// - `GET <path>` — static HTML with the embedded live view
/// - `GET <path>` with `Accept: text/event-stream` — SSE stream
/// - `GET <path>` with WebSocket upgrade headers — WebSocket connection
/// - `POST <path>` with `x-live-view-event: true` — client event
///
/// Merge this into your app with [`Router::merge`]:
///
/// ```rust,ignore
/// let sse = Arc::new(LiveViewSseState::new());
/// let app = Router::new()
///     .merge(page::live_view_page("/", |embed| html! { ... }))
///     .route("/bundle.js", precompiled_js())
///     .layer(Extension(sse));
/// ```
///
/// The SSE state is optional — without it, only WebSocket + static HTML work.
///
/// The client JavaScript automatically tries WebSocket first, then falls
/// back to SSE if the WebSocket connection cannot be established.
///
/// Use the `<meta name="live-view-transport" content="...">` tag to force
/// a specific transport (`"sse"` or `"websocket"`).
///
/// [`Router::merge`]: axum::Router::merge
pub fn live_view_page<L, F>(
    path: &str,
    build_view: F,
) -> axum::Router
where
    L: LiveView,
    F: Fn(EmbedLiveView<'_, L>) -> crate::html::Html<L::Message> + Clone + Send + Sync + 'static,
{
    let build_view = Arc::new(build_view);

    let bv = build_view.clone();
    let handler = move |
        Extension(sse): Extension<Arc<LiveViewSseState>>,
        req: axum::extract::Request,
    | {
        let build_view = bv.clone();
        async move { dispatch_request::<L, F>(sse, build_view, req).await }
    };

    axum::Router::new().route(path, axum::routing::any(handler))
}

// ---------------------------------------------------------------------------
// Request dispatch
// ---------------------------------------------------------------------------

async fn dispatch_request<L, F>(
    sse: Arc<LiveViewSseState>,
    build_view: Arc<F>,
    req: axum::extract::Request,
) -> Response
where
    L: LiveView,
    F: Fn(EmbedLiveView<'_, L>) -> crate::html::Html<L::Message> + Clone + Send + Sync + 'static,
{
    let (parts, body) = req.into_parts();
    let method = parts.method.clone();
    let headers = parts.headers.clone();
    let uri = parts.uri.clone();

    match method {
        Method::GET if is_sse_request(&headers) => {
            handle_sse_stream::<L, F>(&sse, build_view, uri, headers).await
        }
        Method::GET if is_ws_upgrade(&headers) => {
            handle_ws_upgrade::<L, F>(build_view, parts, uri, headers, body).await
        }
        Method::GET => handle_html::<L, F>(build_view).await,
        Method::POST if is_live_view_event(&headers) => {
            let body_bytes = body::to_bytes(body, 1024 * 1024).await.unwrap_or_default();
            handle_event::<L>(&sse, &headers, &body_bytes).await
        }
        _ => StatusCode::METHOD_NOT_ALLOWED.into_response(),
    }
}

// ---------------------------------------------------------------------------
// Request detection
// ---------------------------------------------------------------------------

fn is_sse_request(headers: &HeaderMap) -> bool {
    headers
        .get(http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/event-stream"))
        .unwrap_or(false)
}

fn is_ws_upgrade(headers: &HeaderMap) -> bool {
    headers
        .get(http::header::UPGRADE)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
}

fn is_live_view_event(headers: &HeaderMap) -> bool {
    headers
        .get("x-live-view-event")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Handler: static HTML
// ---------------------------------------------------------------------------

async fn handle_html<L, F>(build_view: Arc<F>) -> Response
where
    L: LiveView,
    F: Fn(EmbedLiveView<'_, L>) -> crate::html::Html<L::Message>,
{
    let embed = EmbedLiveView::noop();
    let html = build_view(embed);
    html.into_response()
}

// ---------------------------------------------------------------------------
// Handler: WebSocket upgrade
// ---------------------------------------------------------------------------

async fn handle_ws_upgrade<L, F>(
    build_view: Arc<F>,
    parts: http::request::Parts,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response
where
    L: LiveView,
    F: Fn(EmbedLiveView<'_, L>) -> crate::html::Html<L::Message>,
{
    let req = axum::extract::Request::from_parts(parts, body);
    match WebSocketUpgrade::from_request(req, &()).await {
        Ok(ws) => {
            let mut view = None;
            let embed = EmbedLiveView::new(&mut view);
            build_view(embed);

            if let Some(view) = view {
                ws.on_upgrade(|socket| run_view_on_socket(socket, view, uri, headers))
                    .into_response()
            } else {
                ws.on_upgrade(|_| async {}).into_response()
            }
        }
        Err(err) => {
            tracing::error!(%err, "WebSocket upgrade failed");
            StatusCode::BAD_REQUEST.into_response()
        }
    }
}

// ---------------------------------------------------------------------------
// Handler: SSE stream
// ---------------------------------------------------------------------------

async fn handle_sse_stream<L, F>(
    sse: &LiveViewSseState,
    build_view: Arc<F>,
    uri: Uri,
    headers: HeaderMap,
) -> Response
where
    L: LiveView,
    F: Fn(EmbedLiveView<'_, L>) -> crate::html::Html<L::Message>,
{
    let connection_id = new_connection_id();

    let mut view = None;
    let embed = EmbedLiveView::new(&mut view);
    build_view(embed);

    let Some(view) = view else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "live_view_page: embed() was not called",
        )
            .into_response();
    };

    let conn_id = ConnectionId(connection_id.clone());
    let (event_tx, event_rx) = tokio::sync::mpsc::channel(256);
    let (update_tx, _) = tokio::sync::broadcast::channel(64);

    let handle = ConnectionHandle {
        event_tx,
        update_tx: update_tx.clone(),
    };

    sse.insert(conn_id.clone(), handle).await;

    tokio::spawn(run_sse_connection(
        view,
        uri,
        headers,
        event_rx,
        update_tx,
        conn_id.clone(),
        sse.clone(),
    ));

    let broadcast_rx = sse
        .get(&conn_id)
        .await
        .expect("connection was just inserted")
        .update_tx
        .subscribe();
    let mpsc_rx = broadcast_to_mpsc(broadcast_rx);

    let stream = SseStream {
        rx: ReceiverStream::new(mpsc_rx),
        state: sse.clone(),
        connection_id: conn_id.clone(),
    };

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("ping"),
        )
        .into_response()
}

// ---------------------------------------------------------------------------
// Handler: POST event
// ---------------------------------------------------------------------------

async fn handle_event<L>(
    sse: &LiveViewSseState,
    headers: &HeaderMap,
    body: &[u8],
) -> Response
where
    L: LiveView,
{
    let connection_id = match headers
        .get("x-live-view-id")
        .and_then(|v| v.to_str().ok())
    {
        Some(id) => ConnectionId(id.to_owned()),
        None => return (StatusCode::BAD_REQUEST, "missing x-live-view-id header").into_response(),
    };

    let payload: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, format!("invalid JSON: {e}")).into_response();
        }
    };

    let raw_event = RawSseEvent {
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

    let handle = match sse.get(&connection_id).await {
        Some(h) => h,
        None => return (StatusCode::NOT_FOUND, "unknown connection id").into_response(),
    };

    match handle.event_tx.send(raw_event).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(_) => (StatusCode::GONE, "connection closed").into_response(),
    }
}
