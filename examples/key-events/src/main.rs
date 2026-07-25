use axum::{response::IntoResponse, Router};
use axum_live_view::{
    event_data::EventData, html, live_page, live_view::Updated, Html, LiveView, LiveViewUpgrade,
};
use serde::{Deserialize, Serialize};
use std::{convert::Infallible, net::SocketAddr};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = axum_live_view::setup(
        Router::new()
            .route("/", live_page(root))

    );

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root(live: LiveViewUpgrade) -> impl IntoResponse {
    let view = View::default();

    live.response(move |embed| {
        html! {
            <!DOCTYPE html>
            <html>
                <head>
                </head>
                <body>
                    { embed.embed(view) }
                    <script src="/_live_view.js"></script>
                </body>
            </html>
        }
    })
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
