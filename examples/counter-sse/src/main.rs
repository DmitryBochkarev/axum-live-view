//! Counter example using `page::live_view_page`.
//!
//! All transport modes (HTML, WebSocket, SSE) are served from the same route.
//! Run with:
//! ```sh
//! cargo run -p example-counter-sse
//! ```

use axum::{Extension, Router};
use axum_live_view::{
    event_data::EventData, html, live_view::Updated,
    page, sse::LiveViewSseState, Html, LiveView,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let sse = Arc::new(LiveViewSseState::new());

    let app = Router::new()
        .merge(page::live_view_page("/", |embed| {
            html! {
                <!DOCTYPE html>
                <html>
                    <head>
                        <meta name="live-view-transport" content="sse"></meta>
                    </head>
                    <body>
                        { embed.embed(Counter::default()) }
                        <script src="/bundle.js"></script>
                    </body>
                </html>
            }
        }))
        .route("/bundle.js", axum_live_view::precompiled_js())
        .layer(Extension(sse));

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

#[derive(Default, Clone)]
struct Counter {
    count: u64,
}

impl LiveView for Counter {
    type Message = Msg;

    fn update(mut self, msg: Msg, _data: Option<EventData>) -> Updated<Self> {
        match msg {
            Msg::Incr => self.count += 1,
            Msg::Decr => {
                if self.count > 0 {
                    self.count -= 1;
                }
            }
        }
        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        html! {
            <div>
                <button axm-click={ Msg::Incr } class="incr-btn">"+"</button>
                <button axm-click={ Msg::Decr } class="decr-btn">"-"</button>
            </div>
            <div class="counter-value">
                { self.count }
            </div>
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Msg {
    Incr,
    Decr,
}
