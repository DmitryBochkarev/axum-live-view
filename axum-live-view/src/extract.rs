//! Extractor for embedding live views in HTML templates.

use crate::{
    html::Html,
    life_cycle::run_view,
    long_poll::{LongPollConnections, LongPollResponse},
    sse::{broadcast_to_mpsc, run_sse_connection, SseStream},
    transport::{ConnectionHandle, ConnectionId, ConnectionRegistry, ServerMessage},
    util::ReceiverStream,
    LiveView,
};
use axum::{
    body::Body,
    extract::{
        ws::{self, WebSocket, WebSocketUpgrade},
        FromRequestParts,
    },
    http::{HeaderMap, StatusCode, Uri},
    response::{
        sse::{KeepAlive, Sse},
        IntoResponse, Response,
    },
};
use futures_util::{
    sink::SinkExt,
    stream::{StreamExt, TryStreamExt},
};
use http::request::Parts;
use std::{convert::Infallible, fmt::Debug, sync::Arc};

pub use crate::life_cycle::EmbedLiveView;

/// Extractor for embedding live views in HTML templates.
///
/// Handles regular HTTP requests (static HTML), WebSocket upgrades, SSE
/// (Server-Sent Events), and long-polling connections transparently.
///
/// To enable SSE and long-polling support, wrap your router with
/// [`setup`](crate::setup):
///
/// ```rust,ignore
/// use axum::{Router, routing::get};
/// use axum_live_view::LiveViewUpgrade;
///
/// let app = axum_live_view::setup(
///     Router::new().route("/", get(root))
/// );
#[derive(Debug)]
pub struct LiveViewUpgrade {
    inner: LiveViewUpgradeInner,
}

#[derive(Debug)]
enum LiveViewUpgradeInner {
    Http,
    Ws(Box<(WebSocketUpgrade, Uri, HeaderMap)>),
    Sse {
        uri: Uri,
        headers: HeaderMap,
        sse: ConnectionRegistry,
    },
    LongPoll {
        uri: Uri,
        headers: HeaderMap,
        sse: ConnectionRegistry,
        lp_connections: LongPollConnections,
    },
}

impl<S> FromRequestParts<S> for LiveViewUpgrade
where
    S: Send + Sync + 'static,
{
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let sse = parts
            .extensions
            .get::<Arc<ConnectionRegistry>>()
            .cloned();
        let lp_connections = parts
            .extensions
            .get::<Arc<LongPollConnections>>()
            .cloned()
            .map(|a| (*a).clone());
        let uri = parts.uri.clone();
        let headers = parts.headers.clone();

        // Check for SSE first (before WebSocket) so the SSE-enabled JS
        // client can request `Accept: text/event-stream` and get an SSE
        // stream even if WebSocket upgrade headers are also present.
        if is_sse_request(&headers) {
            if let Some(sse) = sse {
                return Ok(Self {
                    inner: LiveViewUpgradeInner::Sse {
                        uri,
                        headers,
                        sse: (*sse).clone(),
                    },
                });
            }
        }

        // Check for long-polling after SSE but before WebSocket.
        // The JS client sets `Accept: text/x-live-view-longpoll` when
        // falling back from both WS and SSE.
        if is_long_poll_request(&headers) {
            if let (Some(sse), Some(lp_connections)) = (sse, lp_connections) {
                return Ok(Self {
                    inner: LiveViewUpgradeInner::LongPoll {
                        uri,
                        headers,
                        sse: (*sse).clone(),
                        lp_connections,
                    },
                });
            }
        }

        if let Ok(ws) = WebSocketUpgrade::from_request_parts(parts, state).await {
            Ok(Self {
                inner: LiveViewUpgradeInner::Ws(Box::new((ws, uri, headers))),
            })
        } else {
            Ok(Self {
                inner: LiveViewUpgradeInner::Http,
            })
        }
    }
}

fn is_sse_request(headers: &HeaderMap) -> bool {
    header_accept_contains(headers, "text/event-stream")
}

fn is_long_poll_request(headers: &HeaderMap) -> bool {
    header_accept_contains(headers, "text/x-live-view-longpoll")
}

fn header_accept_contains(headers: &HeaderMap, value: &str) -> bool {
    headers
        .get(http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains(value))
        .unwrap_or(false)
}

impl LiveViewUpgrade {
    /// Return a response that contains an embedded live view.
    ///
    /// This method handles all transport modes transparently:
    /// - Regular `GET` → static HTML (good for SEO and first paint)
    /// - WebSocket upgrade → spawns the view in an async task
    /// - SSE (`Accept: text/event-stream`) → opens an SSE stream
    /// - Long-poll (`Accept: text/x-live-view-longpoll`) → opens a
    ///   long-polling response with initial render + connection ID
    ///
    /// # Example
    ///
    /// ```rust
    /// use axum::response::IntoResponse;
    /// use axum_live_view::{
    ///     event_data::EventData, html, live_view::Updated, Html, LiveView, LiveViewUpgrade,
    /// };
    /// use serde::{Deserialize, Serialize};
    /// use std::convert::Infallible;
    ///
    /// async fn handler(live: LiveViewUpgrade) -> impl IntoResponse {
    ///     live.response(|embed_live_view| {
    ///         html! {
    ///           { embed_live_view.embed(MyView::default()) }
    ///
    ///           // Load the JavaScript. This will automatically initialize live view
    ///           // connections. The /_live_view.js route is registered automatically
    ///           // by axum_live_view::setup().
    ///           <script src="/_live_view.js"></script>
    ///         }
    ///     })
    /// }
    ///
    /// #[derive(Default)]
    /// struct MyView;
    ///
    /// impl LiveView for MyView {
    ///     // ...
    ///     # type Message = Msg;
    ///     # fn update(
    ///     #     mut self,
    ///     #     msg: Msg,
    ///     #     data: Option<EventData>,
    ///     # ) -> Updated<Self> {
    ///     #     todo!()
    ///     # }
    ///     # fn render(&self) -> Html<Self::Message> {
    ///     #     todo!()
    ///     # }
    /// }
    ///
    /// #[derive(Serialize, Deserialize, Debug, PartialEq)]
    /// enum Msg {}
    /// ```
    ///
    /// See the [root module docs](crate) for a more complete example.
    pub fn response<F, L>(self, gather_view: F) -> Response
    where
        L: LiveView,
        F: FnOnce(EmbedLiveView<'_, L>) -> Html<L::Message>,
    {
        match self.inner {
            LiveViewUpgradeInner::Http => {
                let embed = EmbedLiveView::noop();
                gather_view(embed).into_response()
            }
            LiveViewUpgradeInner::Ws(data) => {
                let (ws, uri, headers) = *data;
                let mut view = None;

                let embed = EmbedLiveView::new(&mut view);

                gather_view(embed);

                if let Some(view) = view {
                    ws.on_upgrade(|socket| run_view_on_socket(socket, view, uri, headers))
                        .into_response()
                } else {
                    ws.on_upgrade(|_| async {}).into_response()
                }
            }
            LiveViewUpgradeInner::Sse { uri, headers, sse } => {
                sse_stream_response::<L, F>(sse, gather_view, uri, headers)
            }
            LiveViewUpgradeInner::LongPoll {
                uri,
                headers,
                sse,
                lp_connections,
            } => long_poll_response::<L, F>(sse, lp_connections, gather_view, uri, headers),
        }
    }
}

fn sse_stream_response<L, F>(
    sse: ConnectionRegistry,
    gather_view: F,
    uri: Uri,
    headers: HeaderMap,
) -> Response
where
    L: LiveView,
    F: FnOnce(EmbedLiveView<'_, L>) -> Html<L::Message>,
{
    let connection_id = crate::transport::new_connection_id();
    let conn_id = ConnectionId(connection_id);

    let mut view = None;
    let embed = EmbedLiveView::new(&mut view);
    gather_view(embed);

    let Some(view) = view else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "LiveViewUpgrade: embed() was not called",
        )
            .into_response();
    };

    let (event_tx, event_rx) = tokio::sync::mpsc::channel(256);
    let (update_tx, _) = tokio::sync::broadcast::channel(64);

    let handle = ConnectionHandle {
        event_tx,
        update_tx: update_tx.clone(),
    };

    // Insert the connection handle so POST events can find it
    sse.insert(conn_id.clone(), handle);

    // Spawn the background task that owns the view.
    // Clone everything the task needs before moving.
    let task_update_tx = update_tx.clone();
    let task_sse = sse.clone();
    let task_conn_id = conn_id.clone();
    tokio::spawn(async move {
        run_sse_connection(
            view,
            uri,
            headers,
            event_rx,
            task_update_tx,
            task_conn_id,
            task_sse,
        )
        .await;
    });

    // Subscribe to updates from the broadcast channel we created above.
    // The spawned task holds its own clone of the sender.
    let broadcast_rx = update_tx.subscribe();
    let mpsc_rx = broadcast_to_mpsc(broadcast_rx);

    let stream = SseStream {
        rx: ReceiverStream::new(mpsc_rx),
        state: sse,
        connection_id: conn_id,
    };

    Sse::new(stream)
        .keep_alive(
            KeepAlive::new()
                .interval(std::time::Duration::from_secs(15))
                .text("ping"),
        )
        .into_response()
}

/// Create a long-poll response.
///
/// Two cases:
/// 1. **Initial poll** (no `x-live-view-id` header): calls `gather_view`,
///    spawns the view task, and returns a streaming body that waits for
///    the initial render.
/// 2. **Subsequent poll** (has `x-live-view-id` header): looks up the
///    existing connection and waits for new messages (or timeout).
fn long_poll_response<L, F>(
    sse: ConnectionRegistry,
    lp_connections: LongPollConnections,
    gather_view: F,
    uri: Uri,
    headers: HeaderMap,
) -> Response
where
    L: LiveView,
    F: FnOnce(EmbedLiveView<'_, L>) -> Html<L::Message>,
{
    // Subsequent poll — look up existing connection
    if let Some(conn_id_str) = headers
        .get("x-live-view-id")
        .and_then(|v| v.to_str().ok())
    {
        let conn_id = ConnectionId(conn_id_str.to_owned());
        if !lp_connections.contains(&conn_id) {
            return (StatusCode::GONE, "connection expired").into_response();
        }

        let lp = lp_connections.clone();
        let body_stream = async_stream::stream! {
            match lp
                .wait_for_messages(&conn_id, std::time::Duration::from_secs(30))
                .await
            {
                Some(msgs) => {
                    let messages = if msgs.is_empty() {
                        vec![ServerMessage::Health]
                    } else {
                        msgs
                    };
                    let json = serde_json::to_string(&LongPollResponse::Messages(messages))
                        .unwrap_or_else(|_| "[]".to_owned());
                    yield Ok::<_, Infallible>(bytes::Bytes::from(json));
                }
                None => {
                    // Connection gone
                }
            }
        };

        return Response::builder()
            .header(http::header::CONTENT_TYPE, "application/json")
            .status(StatusCode::OK)
            .body(Body::from_stream(body_stream))
            .unwrap();
    }

    // Initial poll — create the view and spawn the background task
    let mut view = None;
    let embed = EmbedLiveView::new(&mut view);
    gather_view(embed);

    let Some(view) = view else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "LiveViewUpgrade: embed() was not called",
        )
            .into_response();
    };

    let conn_id = crate::long_poll::start_long_poll_connection(
        view,
        uri,
        headers,
        &sse,
        &lp_connections,
    );

    // Return a response with a streaming body. When axum polls the body,
    // it will wait for the view task to push the initial render into the
    // long-poll pending-message buffer.
    let body_stream = async_stream::stream! {
        match lp_connections
            .wait_for_messages(&conn_id, std::time::Duration::from_secs(10))
            .await
        {
            Some(msgs) => {
                let json = serde_json::to_string(&LongPollResponse::Messages(msgs))
                    .unwrap_or_else(|_| "[]".to_owned());
                yield Ok::<_, Infallible>(bytes::Bytes::from(json));
            }
            None => {}
        }
    };

    Response::builder()
        .header(http::header::CONTENT_TYPE, "application/json")
        .status(StatusCode::OK)
        .body(Body::from_stream(body_stream))
        .unwrap()
}

pub(crate) async fn run_view_on_socket<L>(socket: WebSocket, view: L, uri: Uri, headers: HeaderMap)
where
    L: LiveView,
{
    let (write, read) = socket.split();

    let write = write.with(|msg| async move {
        let encoded_msg = ws::Message::Text(serde_json::to_string(&msg)?.into());
        Ok::<_, anyhow::Error>(encoded_msg)
    });
    futures_util::pin_mut!(write);

    let read = read
        .map_err(anyhow::Error::from)
        .and_then(|msg| async move {
            if let ws::Message::Text(text) = msg {
                serde_json::from_str(&text).map_err(Into::into)
            } else {
                anyhow::bail!("received message from socket that wasn't text")
            }
        });
    futures_util::pin_mut!(read);

    if let Err(err) = run_view(write, read, view, uri, headers).await {
        tracing::error!(%err, "encountered while processing socket");
    }
}
