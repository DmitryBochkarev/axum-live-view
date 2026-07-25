use axum::{Extension, Router};
use axum_live_view::{
    sse::LiveViewSseState,
    event_data::EventData, html, live_view::Updated, page, Html, LiveView, 
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    let sse = Arc::new(LiveViewSseState::new());

    let app = Router::new()
        .merge(page::live_view_page("/", |embed| {
            html! {
                <!DOCTYPE html>
                <html>
                    <head></head>
                    <body>
                        { embed.embed(View::default()) }
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
    axum::serve(listener, app).await.unwrap();
}

#[derive(Default, Clone)]
struct View {
    count: u64,
    prev: Option<Msg>,
}

impl LiveView for View {
    type Message = Msg;

    fn update(mut self, msg: Msg, _data: Option<EventData>) -> Updated<Self> {
        self.count += 1;
        self.prev = Some(msg);
        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        html! {
            <div axm-window-keyup={ Msg::Key("window-keyup".to_owned()) } axm-key="escape" >
                <div>
                    "Keydown"
                    <br />
                    <input type="text" axm-keydown={ Msg::Key("keydown".to_owned()) } />
                </div>

                <div>
                    "Keydown (w debounce)"
                    <br />
                    <input
                        type="text"
                        axm-keydown={ Msg::Key("keydown-w-debounce".to_owned()) }
                        axm-debounce="500"
                    />
                </div>

                <div>
                    "Keyup"
                    <br />
                    <input type="text" axm-keyup={ Msg::Key("keyup".to_owned()) }/>
                </div>

                <hr />

                if let Some(event) = &self.prev {
                    <div>"Event count: " { self.count }</div>
                    <pre>
                        <code>
                            { format!("{:#?}", event) }
                        </code>
                    </pre>
                } else {
                    <div>
                        "No keys pressed yet"
                    </div>
                }
            </div>
        }
    }
}

#[derive(Deserialize, Serialize, Debug, PartialEq, Clone)]
enum Msg {
    Key(String),
}
