//! Counter example demonstrating long-poll transport via `LiveViewUpgrade`.
//!
//! Long-poll transport requires:
//! 1. `ConnectionRegistry` in extensions (provided by `axum_live_view::setup`)
//! 2. `LongPollConnections` in extensions (provided by `axum_live_view::setup`)
//! 3. Using `live_page` instead of `get` for the route (adds POST handler)
//! 4. The JS client sends `Accept: text/x-live-view-longpoll` to enable
//!    long-poll mode (triggered by `<meta name="live-view-transport" content="longpoll">`)
//!
//! Run with:
//! ```sh
//! cargo run -p example-counter-longpoll
//! ```

use axum::{Router, response::IntoResponse};
use axum_live_view::{
    Html, LiveView, LiveViewUpgrade, event_data::EventData, html, live_page, live_view::Updated,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = axum_live_view::setup(Router::new().route("/", live_page(root)));

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();
    println!("listening on http://{} (long-poll transport)", addr);
    axum::serve(listener, app).await.unwrap();
}

async fn root(live: LiveViewUpgrade) -> impl IntoResponse {
    live.response(|embed| async move {
        html! {
            <!DOCTYPE html>
            <html>
                <head>
                    // Force long-poll transport via meta tag
                    <meta name="live-view-transport" content="longpoll" />
                </head>
                <body>
                    { embed.embed(Counter::default()) }
                    <script src="/_live_view.js"></script>
                </body>
            </html>
        }
    })
    .await
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
