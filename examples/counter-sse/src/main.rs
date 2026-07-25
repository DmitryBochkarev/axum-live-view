//! Counter example demonstrating SSE transport via `LiveViewUpgrade`.
//!
//! SSE transport requires:
//! 1. `LiveViewSseState` in extensions
//! 2. `sse::event_handler()` for the POST route (client → server events)
//! 3. The JS client sends `Accept: text/event-stream` to enable SSE mode
//!
//! Run with:
//! ```sh
//! cargo run -p example-counter-sse
//! ```

use axum::{Router, response::IntoResponse, routing::get};
use axum_live_view::{
    event_data::EventData, html, live_view::Updated,
    LiveViewUpgrade, Html, LiveView,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = axum_live_view::setup(
        Router::new()
            .route("/", get(root))

    );

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();
}

async fn root(live: LiveViewUpgrade) -> impl IntoResponse {
    live.response(|embed| {
        html! {
            <!DOCTYPE html>
            <html>
                <head>
                    <meta name="live-view-transport" content="sse"></meta>
                </head>
                <body>
                    { embed.embed(Counter::default()) }
                    <script src="/_live_view.js"></script>
                </body>
            </html>
        }
    })
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
