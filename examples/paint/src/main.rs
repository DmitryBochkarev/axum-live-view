use axum::{response::IntoResponse, Router};
use axum_live_view::{
    event_data::EventData, html, live_page, live_view::Updated, Html, LiveView, LiveViewUpgrade,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::net::TcpListener;

const WIDTH: usize = 30;
const HEIGHT: usize = 20;
const CELL_SIZE: usize = 18;

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
    let view = PaintView::default();

    live.response(move |embed| async move {
        html! {
            <!DOCTYPE html>
            <html>
                <head>
                    <style>
                        { STYLE_SHEET }
                    </style>
                </head>
                <body>
                    { embed.embed(view) }
                    <script src="/_live_view.js"></script>
                </body>
            </html>
        }
    }).await
}

const STYLE_SHEET: &str = r#"
    * { margin: 0; padding: 0; box-sizing: border-box; }
    body {
        display: flex;
        justify-content: center;
        align-items: center;
        min-height: 100vh;
        background: #f0f0f0;
        font-family: monospace;
    }
    .paint-app {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 12px;
    }
    .toolbar {
        display: flex;
        align-items: center;
        gap: 10px;
        flex-wrap: wrap;
        justify-content: center;
    }
    .toolbar-label {
        font-weight: bold;
    }
    .color-btn {
        width: 28px;
        height: 28px;
        cursor: pointer;
        border-radius: 4px;
        outline: none;
        border-width: 2px;
        border-style: solid;
    }
    .color-label {
        margin-left: 12px;
        font-size: 13px;
        color: #666;
    }
    .clear-btn {
        margin-left: 16px;
        padding: 6px 14px;
        cursor: pointer;
        background: #fff;
        border-width: 1px;
        border-style: solid;
        border-color: #ccc;
        border-radius: 4px;
        font-family: monospace;
    }
    .hint {
        font-size: 11px;
        color: #999;
    }
"#;

#[derive(Clone)]
struct PaintView {
    pixels: Vec<Vec<Color>>,
    selected_color: Color,
}

impl Default for PaintView {
    fn default() -> Self {
        Self {
            pixels: vec![vec![Color::White; WIDTH]; HEIGHT],
            selected_color: Color::Black,
        }
    }
}

impl LiveView for PaintView {
    type Message = Msg;

    fn update(mut self, msg: Msg, _data: Option<EventData>) -> Updated<Self> {
        match msg {
            Msg::Paint { x, y } => {
                if x < WIDTH && y < HEIGHT {
                    self.pixels[y][x] = self.selected_color;
                }
            }
            Msg::SelectColor(color) => {
                self.selected_color = color;
            }
            Msg::Clear => {
                for row in &mut self.pixels {
                    for cell in row {
                        *cell = Color::White;
                    }
                }
            }
        }

        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        html! {
            <div class="paint-app">

                <div class="toolbar">

                    <span class="toolbar-label">"Color:"</span>

                    for color in Color::all() {
                        <button
                            axm-click={ Msg::SelectColor(color) }
                            class="color-btn"
                            style={
                                if self.selected_color == color {
                                    format!(
                                        "border-color:#333;background:{};",
                                        color.css()
                                    )
                                } else {
                                    format!(
                                        "border-color:#ccc;background:{};",
                                        color.css()
                                    )
                                }
                            }
                            title={ color.name() }
                        ></button>
                    }

                    // <span class="color-label">
                    //     { self.selected_color.name() }
                    // </span>

                    <button
                        axm-click={ Msg::Clear }
                        class="clear-btn"
                    >
                        "Clear"
                    </button>
                </div>

                <div
                    style={
                        format!(
                            "display:grid;\
                             grid-template-columns:repeat({},{}px);\
                             grid-template-rows:repeat({},{}px);\
                             border-width:1px;border-style:solid;border-color:#999;\
                             cursor:crosshair;user-select:none;",
                            WIDTH, CELL_SIZE, HEIGHT, CELL_SIZE
                        )
                    }
                >
                    for y in 0..HEIGHT {
                        for x in 0..WIDTH {
                            <div
                                axm-click={ Msg::Paint { x, y } }
                                style={
                                    format!(
                                        "border-width:1px;border-style:solid;border-color:#eee;\
                                         width:{}px;height:{}px;background:{};",
                                        CELL_SIZE, CELL_SIZE,
                                        self.pixels[y][x].css()
                                    )
                                }
                            ></div>
                        }
                    }
                </div>

                <div class="hint">
                    "Click or drag to paint. Select a color from the palette above."
                </div>
            </div>
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
enum Msg {
    Paint { x: usize, y: usize },
    SelectColor(Color),
    Clear,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
enum Color {
    Black,
    White,
    Red,
    Green,
    Blue,
    Yellow,
    Orange,
    Purple,
    Cyan,
    Gray,
}

impl Color {
    fn all() -> [Color; 10] {
        [
            Color::Black,
            Color::White,
            Color::Red,
            Color::Green,
            Color::Blue,
            Color::Yellow,
            Color::Orange,
            Color::Purple,
            Color::Cyan,
            Color::Gray,
        ]
    }

    fn css(&self) -> &'static str {
        match self {
            Color::Black => "#000000",
            Color::White => "#ffffff",
            Color::Red => "#e74c3c",
            Color::Green => "#2ecc71",
            Color::Blue => "#3498db",
            Color::Yellow => "#f1c40f",
            Color::Orange => "#e67e22",
            Color::Purple => "#9b59b6",
            Color::Cyan => "#1abc9c",
            Color::Gray => "#95a5a6",
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Color::Black => "Black",
            Color::White => "Eraser",
            Color::Red => "Red",
            Color::Green => "Green",
            Color::Blue => "Blue",
            Color::Yellow => "Yellow",
            Color::Orange => "Orange",
            Color::Purple => "Purple",
            Color::Cyan => "Cyan",
            Color::Gray => "Gray",
        }
    }
}
