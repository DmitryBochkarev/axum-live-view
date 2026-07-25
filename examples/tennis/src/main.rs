use axum::{
    extract::State,
    http::{header, HeaderMap, Uri},
    response::IntoResponse,
    routing::get,
    Router,
};
use axum_live_view::{
    event_data::EventData,
    html, live_page,
    live_view::{Updated, ViewHandle},
    Html, LiveView, LiveViewUpgrade,
};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::{Arc, RwLock},
};
use tokio::{net::TcpListener, sync::broadcast};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let (tx, _) = broadcast::channel::<RefreshPing>(1024);

    let state = AppState {
        data: Arc::new(RwLock::new(TennisData::default())),
        tx,
    };

    let app = axum_live_view::setup(
        Router::new()
            .route("/", live_page(root))
            .route("/observe", live_page(observe))

            .route("/xp.css", get(xp_css))
            .route("/ms_sans_serif.woff", get(ms_sans_serif_woff))
            .route("/ms_sans_serif.woff2", get(ms_sans_serif_woff2))
            .with_state(state)
    );

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ---------------------------------------------------------------------------
// Global state stored in the axum server, shared across all connections.
// ---------------------------------------------------------------------------

/// Holds all data that is common across browsers.
#[derive(Clone, Default, Debug)]
struct TennisData {
    matches: Vec<Match>,
    next_id: u64,
}

/// axum application state. Cloned per-request.
#[derive(Clone)]
struct AppState {
    data: Arc<RwLock<TennisData>>,
    tx: broadcast::Sender<RefreshPing>,
}

/// Sent on every mutation so every connected browser re-renders.
#[derive(Clone, Copy, Debug)]
struct RefreshPing;

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Admin page – create matches, add points.
async fn root(
    live: LiveViewUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let view = TennisApp {
        data: state.data.clone(),
        tx: state.tx.clone(),
        player_1: String::new(),
        player_2: String::new(),
    };

    live.response(move |embed| {
        html! {
            <!DOCTYPE html>
            <html>
                <head>
                    <title>"Tennis – Admin"</title>
                    <link rel="stylesheet" href="/xp.css" />
                </head>
                <body>
                    <div class="window" style="max-width: 840px; margin: 2rem auto;">
                        <div class="title-bar">
                            <div class="title-bar-text">"🎾 Tennis Admin"</div>
                            <div class="title-bar-controls">
                                <a href="/observe">
                                    <button aria-label="Observer view" style="min-width: auto;">"👀 Observer view"</button>
                                </a>
                            </div>
                        </div>
                        <div class="window-body">
                            { embed.embed(view) }
                        </div>
                    </div>
                    <script src="/_live_view.js"></script>
                </body>
            </html>
        }
    })
}

/// Read-only observer page – see scores update in real time, no controls.
async fn observe(
    live: LiveViewUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let view = ObserverApp {
        data: state.data.clone(),
        tx: state.tx.clone(),
    };

    live.response(move |embed| {
        html! {
            <!DOCTYPE html>
            <html>
                <head>
                    <title>"Tennis – Live Scoreboard"</title>
                    <link rel="stylesheet" href="/xp.css" />
                </head>
                <body>
                    <div class="window" style="max-width: 740px; margin: 2rem auto;">
                        <div class="title-bar">
                            <div class="title-bar-text">"🎾 Live Scoreboard"</div>
                            <div class="title-bar-controls">
                                <a href="/">
                                    <button aria-label="Admin" style="min-width: auto;">"⚙️ Admin"</button>
                                </a>
                            </div>
                        </div>
                        <div class="window-body">
                            { embed.embed(view) }
                        </div>
                    </div>
                    <script src="/_live_view.js"></script>
                </body>
            </html>
        }
    })
}

// ---------------------------------------------------------------------------
// Static asset handlers
// ---------------------------------------------------------------------------

async fn xp_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], include_str!("../assets/xp.css"))
}

async fn ms_sans_serif_woff() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "font/woff")],
        include_bytes!("../assets/ms_sans_serif.woff").as_ref(),
    )
}

async fn ms_sans_serif_woff2() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "font/woff2")],
        include_bytes!("../assets/ms_sans_serif.woff2").as_ref(),
    )
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct Player {
    name: String,
    points: u32,
}

impl Player {
    fn new(name: String) -> Self {
        Self { name, points: 0 }
    }
}

#[derive(Clone, Debug)]
struct Match {
    id: u64,
    player_1: Player,
    player_2: Player,
    finished: bool,
}

// ---------------------------------------------------------------------------
// Per-connection view
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct TennisApp {
    data: Arc<RwLock<TennisData>>,
    tx: broadcast::Sender<RefreshPing>,
    player_1: String,
    player_2: String,
}

impl TennisApp {
    /// Convenience constructor for tests.
    #[cfg(test)]
    fn new_test() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            data: Arc::new(RwLock::new(TennisData::default())),
            tx,
            player_1: String::new(),
            player_2: String::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
enum PlayerNum {
    #[serde(rename = "player_1")]
    One,
    #[serde(rename = "player_2")]
    Two,
}

impl LiveView for TennisApp {
    type Message = Msg;

    /// Subscribe to global refresh pings. Whenever *any* browser changes the
    /// shared state we receive [`Msg::Refresh`] and re-render.
    fn mount(
        &mut self,
        _uri: Uri,
        _request_headers: &HeaderMap,
        handle: ViewHandle<Self::Message>,
    ) {
        let mut rx = self.tx.subscribe();
        tokio::spawn(async move {
            while let Ok(RefreshPing) = rx.recv().await {
                if handle.send(Msg::Refresh).await.is_err() {
                    break;
                }
            }
        });
    }

    fn update(mut self, msg: Msg, data: Option<EventData>) -> Updated<Self> {
        match msg {
            // Internal message triggered by the broadcast channel – just
            // re-render with whatever the shared state contains right now.
            Msg::Refresh => {}

            Msg::Player1Input => {
                if let Some(input) = data.and_then(|d| d.as_input().cloned()) {
                    if let Some(value) = input.as_str() {
                        self.player_1 = value.to_owned();
                    }
                }
            }
            Msg::Player2Input => {
                if let Some(input) = data.and_then(|d| d.as_input().cloned()) {
                    if let Some(value) = input.as_str() {
                        self.player_2 = value.to_owned();
                    }
                }
            }
            Msg::CreateMatch => {
                if let Some(form) = data.and_then(|d| d.as_form().cloned()) {
                    if let Ok(values) = form.deserialize::<NewMatchFormData>() {
                        let p1 = values.player_1.trim().to_owned();
                        let p2 = values.player_2.trim().to_owned();
                        if !p1.is_empty() && !p2.is_empty() {
                            // Modify the shared state …
                            {
                                let mut data = self.data.write().unwrap();
                                let id = data.next_id;
                                data.next_id += 1;
                                data.matches.insert(
                                    0,
                                    Match {
                                        id,
                                        player_1: Player::new(p1),
                                        player_2: Player::new(p2),
                                        finished: false,
                                    },
                                );
                            }
                            self.player_1.clear();
                            self.player_2.clear();
                            // … then tell every connected browser (including this one).
                            let _ = self.tx.send(RefreshPing);
                        }
                    }
                }
            }
            Msg::AddPoint(id, player_num) => {
                {
                    let mut data = self.data.write().unwrap();
                    if let Some(m) = data.matches.iter_mut().find(|m| m.id == id) {
                        match player_num {
                            PlayerNum::One => m.player_1.points += 1,
                            PlayerNum::Two => m.player_2.points += 1,
                        }
                    }
                }
                let _ = self.tx.send(RefreshPing);
            }
            Msg::FinishMatch(id) => {
                {
                    let mut data = self.data.write().unwrap();
                    if let Some(m) = data.matches.iter_mut().find(|m| m.id == id) {
                        m.finished = true;
                    }
                }
                let _ = self.tx.send(RefreshPing);
            }
        }

        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        let data = self.data.read().unwrap();
        let form_is_valid =
            !self.player_1.trim().is_empty() && !self.player_2.trim().is_empty();

        let match_colors: Vec<(&str, &str)> = data
            .matches
            .iter()
            .map(|m| {
                if m.finished {
                    if m.player_1.points > m.player_2.points {
                        ("color: green; font-weight: bold;", "color: red;")
                    } else if m.player_2.points > m.player_1.points {
                        ("color: red;", "color: green; font-weight: bold;")
                    } else {
                        ("", "")
                    }
                } else {
                    ("", "")
                }
            })
            .collect();

        html! {
            <form axm-submit={ Msg::CreateMatch }>
                <div class="field-row">
                    <input
                        type="text"
                        name="player_1"
                        value={ &self.player_1 }
                        placeholder="Player one name"
                        axm-input={ Msg::Player1Input }
                    />
                    <input
                        type="text"
                        name="player_2"
                        value={ &self.player_2 }
                        placeholder="Player two name"
                        axm-input={ Msg::Player2Input }
                    />
                </div>
                <div class="field-row" style="margin-top: 8px;">
                    <button disabled=if form_is_valid { None } else { Some(()) }>"Create Match"</button>
                </div>
            </form>

            <div class="field-row" style="margin-top: 24px; margin-bottom: 8px;">
                <span style="font-weight: bold; font-size: 14px;">"Tennis Matches"</span>
            </div>

            if data.matches.is_empty() {
                <div class="sunken-panel" style="padding: 32px; text-align: center;">
                    <p><i>"Create some and they will be listed here."</i></p>
                </div>
            }

            for (i, m) in data.matches.iter().enumerate() {
                <div class="field-row" style="margin-bottom: 10px;">
                    <div class="window" style="flex: 1;">
                        <div class="window-body">
                            <div class="field-row" style="justify-content: space-around; text-align: center;">
                                <div style="flex: 1;">
                                    <p style={ match_colors[i].0 }>{ &m.player_1.name }</p>
                                    <p>"Points: " { m.player_1.points }</p>
                                    if !m.finished {
                                        <button axm-click={ Msg::AddPoint(m.id, PlayerNum::One) }>"+ point"</button>
                                    }
                                </div>
                                <div style="flex: 1;">
                                    <p style={ match_colors[i].1 }>{ &m.player_2.name }</p>
                                    <p>"Points: " { m.player_2.points }</p>
                                    if !m.finished {
                                        <button axm-click={ Msg::AddPoint(m.id, PlayerNum::Two) }>"+ point"</button>
                                    }
                                </div>
                            </div>
                            <div class="field-row" style="justify-content: center; margin-top: 10px;">
                                if m.finished {
                                    <p>"Finished"</p>
                                } else {
                                    <button axm-click={ Msg::FinishMatch(m.id) }>"Finish Match"</button>
                                }
                            </div>
                        </div>
                    </div>
                </div>
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Read-only observer view
// ---------------------------------------------------------------------------

/// Shared-state backed view with no interactive controls – for observers.
#[derive(Clone, Debug)]
struct ObserverApp {
    data: Arc<RwLock<TennisData>>,
    tx: broadcast::Sender<RefreshPing>,
}

impl ObserverApp {
    #[cfg(test)]
    fn new_test() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self {
            data: Arc::new(RwLock::new(TennisData::default())),
            tx,
        }
    }
}

impl LiveView for ObserverApp {
    type Message = ObserverMsg;

    fn mount(
        &mut self,
        _uri: Uri,
        _request_headers: &HeaderMap,
        handle: ViewHandle<Self::Message>,
    ) {
        let mut rx = self.tx.subscribe();
        tokio::spawn(async move {
            while let Ok(RefreshPing) = rx.recv().await {
                if handle.send(ObserverMsg::Refresh).await.is_err() {
                    break;
                }
            }
        });
    }

    fn update(self, msg: ObserverMsg, _data: Option<EventData>) -> Updated<Self> {
        match msg {
            ObserverMsg::Refresh => {}
        }
        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        let data = self.data.read().unwrap();

        let match_colors: Vec<(&str, &str)> = data
            .matches
            .iter()
            .map(|m| {
                if m.finished {
                    if m.player_1.points > m.player_2.points {
                        ("color: green; font-weight: bold;", "color: red;")
                    } else if m.player_2.points > m.player_1.points {
                        ("color: red;", "color: green; font-weight: bold;")
                    } else {
                        ("", "")
                    }
                } else {
                    ("", "")
                }
            })
            .collect();

        html! {
            if data.matches.is_empty() {
                <div style="text-align: center; padding: 48px 0;">
                    <p style="font-size: 14px;">"No matches in progress."</p>
                    <p style="margin-top: 8px;">
                        "Head over to the " <a href="/">"Admin page"</a> " to create one."
                    </p>
                </div>
            } else {
                for (i, m) in data.matches.iter().enumerate() {
                    <div class="window" style="margin-bottom: 12px;">
                        <div class="window-body">
                            <div class="field-row" style="justify-content: center; align-items: center;">
                                <div style="flex: 1; text-align: right; padding-right: 16px;">
                                    <p style={ match_colors[i].0 }>{ &m.player_1.name }</p>
                                </div>
                                <div class="field-row" style="align-items: center; gap: 8px;">
                                    <div class="sunken-panel" style="padding: 4px 14px; text-align: center; min-width: 44px;">
                                        <span style="font-size: 22px; font-weight: bold;">{ m.player_1.points }</span>
                                    </div>
                                    <span style="font-size: 18px;">":"</span>
                                    <div class="sunken-panel" style="padding: 4px 14px; text-align: center; min-width: 44px;">
                                        <span style="font-size: 22px; font-weight: bold;">{ m.player_2.points }</span>
                                    </div>
                                </div>
                                <div style="flex: 1; text-align: left; padding-left: 16px;">
                                    <p style={ match_colors[i].1 }>{ &m.player_2.name }</p>
                                </div>
                            </div>
                        </div>
                    </div>
                }
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
enum ObserverMsg {
    Refresh,
}

// ---------------------------------------------------------------------------
// Messages & form data
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Msg {
    /// Internal: re-render because another browser changed shared state.
    Refresh,
    Player1Input,
    Player2Input,
    CreateMatch,
    AddPoint(u64, PlayerNum),
    FinishMatch(u64),
}

#[derive(Debug, Serialize, Deserialize)]
struct NewMatchFormData {
    player_1: String,
    player_2: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum_live_view::{event_data, test::run_live_view};

    /// Helper to build an input event with a string value.
    fn input_event(s: &str) -> Option<EventData> {
        Some(EventData::Input(event_data::Input::String(s.to_owned())))
    }

    /// Helper to build a form submit event.
    fn form_submit(player_1: &str, player_2: &str) -> Option<EventData> {
        let form = event_data::Form::builder()
            .serialize(&NewMatchFormData {
                player_1: player_1.to_owned(),
                player_2: player_2.to_owned(),
            })
            .unwrap()
            .build();
        Some(EventData::Form(form))
    }

    #[tokio::test]
    async fn initial_render() {
        let view = run_live_view(TennisApp::new_test()).mount().await;
        let html = view.render().await;

        assert!(html.contains("Tennis Matches"));
        assert!(html.contains("Create some and they will be listed here."));
        assert!(html.contains("Player one name"));
        assert!(html.contains("Player two name"));
    }

    // -- Observer tests -------------------------------------------------------

    #[tokio::test]
    async fn observer_shows_empty_state() {
        let view = run_live_view(ObserverApp::new_test()).mount().await;
        let html = view.render().await;

        assert!(html.contains("No matches in progress."));
        assert!(!html.contains("button"));
    }

    #[tokio::test]
    async fn observer_sees_match_created_by_admin() {
        let (tx, _) = broadcast::channel(1024);
        let data = Arc::new(RwLock::new(TennisData::default()));

        let admin = TennisApp {
            data: data.clone(),
            tx: tx.clone(),
            player_1: String::new(),
            player_2: String::new(),
        };
        let observer = ObserverApp {
            data: data.clone(),
            tx: tx.clone(),
        };

        let admin_h = run_live_view(admin).mount().await;
        let obs_h = run_live_view(observer).mount().await;

        // Admin creates a match.
        admin_h
            .send(Msg::Player1Input, input_event("Serena"))
            .await;
        admin_h
            .send(Msg::Player2Input, input_event("Venus"))
            .await;
        admin_h
            .send(Msg::CreateMatch, form_submit("Serena", "Venus"))
            .await;

        // Observer sees it after a refresh.
        let (obs_html, _) = obs_h.send(ObserverMsg::Refresh, None).await;
        assert!(obs_html.contains("Serena"));
        assert!(obs_html.contains("Venus"));
        // No interactive elements.
        assert!(!obs_html.contains("button"));
        assert!(!obs_html.contains("axm-click"));
    }

    #[tokio::test]
    async fn observer_sees_score_updates() {
        let (tx, _) = broadcast::channel(1024);
        let data = Arc::new(RwLock::new(TennisData::default()));

        let admin = TennisApp {
            data: data.clone(),
            tx: tx.clone(),
            player_1: String::new(),
            player_2: String::new(),
        };
        let observer = ObserverApp {
            data: data.clone(),
            tx: tx.clone(),
        };

        let admin_h = run_live_view(admin).mount().await;
        let obs_h = run_live_view(observer).mount().await;

        // Create match as admin.
        admin_h
            .send(Msg::Player1Input, input_event("Roger"))
            .await;
        admin_h
            .send(Msg::Player2Input, input_event("Rafa"))
            .await;
        admin_h
            .send(Msg::CreateMatch, form_submit("Roger", "Rafa"))
            .await;

        // Add points.
        admin_h
            .send(Msg::AddPoint(0, PlayerNum::One), None)
            .await;
        admin_h
            .send(Msg::AddPoint(0, PlayerNum::One), None)
            .await;
        admin_h
            .send(Msg::AddPoint(0, PlayerNum::Two), None)
            .await;

        // Observer sees the updated score.
        let (obs_html, _) = obs_h.send(ObserverMsg::Refresh, None).await;
        assert!(obs_html.contains(">2<"));
        assert!(obs_html.contains(">1<"));
    }

    #[tokio::test]
    async fn form_input_updates_player_names() {
        let view = run_live_view(TennisApp::new_test()).mount().await;

        view.send(Msg::Player1Input, input_event("Roger")).await;
        let (html, _) = view.send(Msg::Player2Input, input_event("Rafa")).await;

        assert!(html.contains("Roger"));
        assert!(html.contains("Rafa"));
    }

    #[tokio::test]
    async fn submit_button_enabled_only_when_form_valid() {
        let view = run_live_view(TennisApp::new_test()).mount().await;

        let html = view.render().await;
        assert!(html.contains("disabled"));

        let (html, _) = view
            .send(Msg::Player1Input, input_event("Roger"))
            .await;
        assert!(html.contains("disabled"));

        let (html, _) = view
            .send(Msg::Player2Input, input_event("Rafa"))
            .await;
        assert!(!html.contains("disabled"));
    }

    #[tokio::test]
    async fn create_match() {
        let view = run_live_view(TennisApp::new_test()).mount().await;

        view.send(Msg::Player1Input, input_event("Roger")).await;
        view.send(Msg::Player2Input, input_event("Rafa")).await;

        let (html, _) = view
            .send(Msg::CreateMatch, form_submit("Roger", "Rafa"))
            .await;

        assert!(html.contains("Roger"));
        assert!(html.contains("Rafa"));
        assert!(!html.contains("Create some and they will be listed here."));
        assert!(!html.contains(r#"value="Roger""#));
        assert!(!html.contains(r#"value="Rafa""#));
    }

    #[tokio::test]
    async fn create_multiple_matches() {
        let view = run_live_view(TennisApp::new_test()).mount().await;

        for (p1, p2) in [("Roger", "Rafa"), ("Novak", "Andy")] {
            view.send(Msg::Player1Input, input_event(p1)).await;
            view.send(Msg::Player2Input, input_event(p2)).await;
            view.send(Msg::CreateMatch, form_submit(p1, p2)).await;
        }

        let html = view.render().await;
        let roger_pos = html.find("Roger").unwrap();
        let novak_pos = html.find("Novak").unwrap();
        assert!(novak_pos < roger_pos);
    }

    #[tokio::test]
    async fn add_point_to_player() {
        let view = run_live_view(TennisApp::new_test()).mount().await;

        view.send(Msg::Player1Input, input_event("Roger")).await;
        view.send(Msg::Player2Input, input_event("Rafa")).await;
        view.send(Msg::CreateMatch, form_submit("Roger", "Rafa")).await;

        let (html, _) = view.send(Msg::AddPoint(0, PlayerNum::One), None).await;
        assert!(html.contains("Points: 1"));

        let (html, _) = view.send(Msg::AddPoint(0, PlayerNum::One), None).await;
        assert!(html.contains("Points: 2"));

        let (html, _) = view.send(Msg::AddPoint(0, PlayerNum::Two), None).await;
        assert!(html.contains("Points: 1"));
    }

    #[tokio::test]
    async fn empty_form_does_not_create_match() {
        let view = run_live_view(TennisApp::new_test()).mount().await;

        let (html, _) = view
            .send(Msg::CreateMatch, form_submit("", ""))
            .await;

        assert!(html.contains("Create some and they will be listed here."));
    }

    #[tokio::test]
    async fn whitespace_only_names_rejected() {
        let view = run_live_view(TennisApp::new_test()).mount().await;

        view.send(Msg::Player1Input, input_event("   ")).await;
        view.send(Msg::Player2Input, input_event("   ")).await;
        let (html, _) = view
            .send(Msg::CreateMatch, form_submit("   ", "   "))
            .await;

        assert!(html.contains("Create some and they will be listed here."));
    }

    /// Verify that two separate views see the same shared state.
    #[tokio::test]
    async fn two_views_share_state() {
        let (tx, _) = broadcast::channel(1024);
        let data = Arc::new(RwLock::new(TennisData::default()));

        let view1 = TennisApp {
            data: data.clone(),
            tx: tx.clone(),
            player_1: String::new(),
            player_2: String::new(),
        };
        let view2 = TennisApp {
            data: data.clone(),
            tx: tx.clone(),
            player_1: String::new(),
            player_2: String::new(),
        };

        let handle1 = run_live_view(view1).mount().await;
        let handle2 = run_live_view(view2).mount().await;

        // View 1 creates a match.
        handle1
            .send(Msg::Player1Input, input_event("Roger"))
            .await;
        handle1
            .send(Msg::Player2Input, input_event("Rafa"))
            .await;
        handle1
            .send(Msg::CreateMatch, form_submit("Roger", "Rafa"))
            .await;

        // View 2 should see it immediately (it re-renders on its own from the
        // broadcast when Refresh spawns are run). But in test mode spawns are
        // dropped, so we send Refresh manually.
        let (html2, _) = handle2.send(Msg::Refresh, None).await;
        assert!(html2.contains("Roger"));
        assert!(html2.contains("Rafa"));

        // View 2 adds a point.
        handle2
            .send(Msg::AddPoint(0, PlayerNum::One), None)
            .await;
        let (html2, _) = handle2.send(Msg::Refresh, None).await;
        assert!(html2.contains("Points: 1"));

        // View 1 sees the point too.
        let (html1, _) = handle1.send(Msg::Refresh, None).await;
        assert!(html1.contains("Points: 1"));
    }
}
