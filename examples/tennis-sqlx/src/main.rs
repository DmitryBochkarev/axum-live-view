use axum::{
    Router,
    extract::State,
    http::{HeaderMap, Uri, header},
    response::IntoResponse,
    routing::get,
};
use axum_live_view::{
    Html, LiveView, LiveViewUpgrade,
    event_data::EventData,
    html, live_page,
    live_view::{Updated, ViewHandle},
};
use serde::{Deserialize, Serialize};
use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};
use std::net::SocketAddr;
use tokio::{net::TcpListener, sync::broadcast};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // Set up the SQLite database. Uses a file so data persists across restarts.
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite:tennis.db".to_owned());
    let options: SqliteConnectOptions = db_url.parse()?;
    let pool = SqlitePool::connect_with(options.create_if_missing(true)).await?;
    init_db(&pool).await?;

    let (tx, _) = broadcast::channel::<RefreshPing>(1024);

    let state = AppState { db: pool, tx };

    let app = axum_live_view::setup(
        Router::new()
            .route("/", live_page(root))
            .route("/observe", live_page(observe))
            .route("/xp.css", get(xp_css))
            .route("/ms_sans_serif.woff", get(ms_sans_serif_woff))
            .route("/ms_sans_serif.woff2", get(ms_sans_serif_woff2))
            .with_state(state),
    );

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Database helpers
// ---------------------------------------------------------------------------

async fn init_db(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS matches (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            player_1_name TEXT NOT NULL,
            player_1_points INTEGER NOT NULL DEFAULT 0,
            player_2_name TEXT NOT NULL,
            player_2_points INTEGER NOT NULL DEFAULT 0,
            finished INTEGER NOT NULL DEFAULT 0
        )",
    )
    .execute(pool)
    .await?;
    Ok(())
}

async fn fetch_matches(db: &SqlitePool) -> Result<Vec<Match>, sqlx::Error> {
    sqlx::query_as::<_, Match>(
        "SELECT id, player_1_name, player_1_points, player_2_name, player_2_points, finished
         FROM matches ORDER BY id DESC",
    )
    .fetch_all(db)
    .await
}

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct AppState {
    db: SqlitePool,
    tx: broadcast::Sender<RefreshPing>,
}

/// Sent on every mutation so every connected browser re-renders.
#[derive(Clone, Copy, Debug)]
struct RefreshPing;

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// A tennis match, mapped directly from the `matches` table row.
#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow, PartialEq, Eq)]
struct Match {
    id: i64,
    player_1_name: String,
    player_1_points: i64,
    player_2_name: String,
    player_2_points: i64,
    finished: i64, // 0 = active, 1 = finished
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// Admin page – create matches, add points.
async fn root(live: LiveViewUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    live.response(|embed| async move {
        let mut view = TennisApp::new(state);
        view.matches = fetch_matches(&view.state.db).await.unwrap_or_default();

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
    }).await
}

/// Read-only observer page – see scores update in real time, no controls.
async fn observe(live: LiveViewUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    live.response(|embed| async move {
        let mut view = ObserverApp::new(state);
        view.matches = fetch_matches(&view.state.db).await.unwrap_or_default();

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
    }).await
}

// ---------------------------------------------------------------------------
// Static asset handlers
// ---------------------------------------------------------------------------

async fn xp_css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../assets/xp.css"),
    )
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
// Admin view
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct TennisApp {
    state: AppState,
    /// Local snapshot of all matches for `render`.
    matches: Vec<Match>,
    player_1: String,
    player_2: String,
}

impl TennisApp {
    fn new(state: AppState) -> Self {
        Self {
            state,
            matches: Vec::new(),
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

    fn mount(
        &mut self,
        _uri: Uri,
        _request_headers: &HeaderMap,
        handle: ViewHandle<Self::Message>,
    ) {
        let mut rx = self.state.tx.subscribe();
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
            Msg::Noop => {}

            Msg::Refresh => {
                let db = self.state.db.clone();
                return Updated::new(self).spawn(async move {
                    match fetch_matches(&db).await {
                        Ok(matches) => Msg::MatchesList(matches),
                        Err(e) => {
                            tracing::error!("failed to fetch matches: {e}");
                            Msg::Noop
                        }
                    }
                });
            }

            Msg::Player1Input => {
                if let Some(input) = data.and_then(|d| d.as_input().cloned())
                    && let Some(value) = input.as_str()
                {
                    self.player_1 = value.to_owned();
                }
            }
            Msg::Player2Input => {
                if let Some(input) = data.and_then(|d| d.as_input().cloned())
                    && let Some(value) = input.as_str()
                {
                    self.player_2 = value.to_owned();
                }
            }
            Msg::CreateMatch => {
                if let Some(form) = data.and_then(|d| d.as_form().cloned())
                    && let Ok(values) = form.deserialize::<NewMatchFormData>()
                {
                    let p1 = values.player_1.trim().to_owned();
                    let p2 = values.player_2.trim().to_owned();
                    if !p1.is_empty() && !p2.is_empty() {
                        self.player_1.clear();
                        self.player_2.clear();

                        let tx = self.state.tx.clone();
                        let db = self.state.db.clone();
                        return Updated::new(self).spawn(async move {
                            if let Err(e) = sqlx::query(
                                "INSERT INTO matches (player_1_name, player_2_name) VALUES (?, ?)",
                            )
                            .bind(&p1)
                            .bind(&p2)
                            .execute(&db)
                            .await
                            {
                                tracing::error!("failed to insert match: {e}");
                                return Msg::Noop;
                            }

                            let _ = tx.send(RefreshPing);

                            Msg::Noop
                        });
                    }
                }
            }
            Msg::AddPoint(id, player_num) => {
                let tx = self.state.tx.clone();
                let db = self.state.db.clone();
                return Updated::new(self).spawn(async move {
                    let result = match player_num {
                        PlayerNum::One => sqlx::query(
                            "UPDATE matches SET player_1_points = player_1_points + 1 WHERE id = ?",
                        )
                        .bind(id)
                        .execute(&db)
                        .await,
                        PlayerNum::Two => sqlx::query(
                            "UPDATE matches SET player_2_points = player_2_points + 1 WHERE id = ?",
                        )
                        .bind(id)
                        .execute(&db)
                        .await,
                    };
                    if let Err(e) = result {
                        tracing::error!("failed to update points: {e}");
                        return Msg::Noop;
                    }
                    let _ = tx.send(RefreshPing);
                    Msg::Noop
                });
            }
            Msg::FinishMatch(id) => {
                let db = self.state.db.clone();
                let tx = self.state.tx.clone();
                return Updated::new(self).spawn(async move {
                    if let Err(e) = sqlx::query("UPDATE matches SET finished = 1 WHERE id = ?")
                        .bind(id)
                        .execute(&db)
                        .await
                    {
                        tracing::error!("failed to finish match: {e}");
                        return Msg::Noop;
                    }
                    let _ = tx.send(RefreshPing);
                    Msg::Noop
                });
            }
            Msg::MatchesList(items) => self.matches = items,
        }

        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        let form_is_valid = !self.player_1.trim().is_empty() && !self.player_2.trim().is_empty();

        let match_colors: Vec<(&str, &str)> = self
            .matches
            .iter()
            .map(|m| {
                if m.finished != 0 {
                    if m.player_1_points > m.player_2_points {
                        ("color: green; font-weight: bold;", "color: red;")
                    } else if m.player_2_points > m.player_1_points {
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

            if self.matches.is_empty() {
                <div class="sunken-panel" style="padding: 32px; text-align: center;">
                    <p><i>"Create some and they will be listed here."</i></p>
                </div>
            }

            for (i, m) in self.matches.iter().enumerate() {
                <div class="field-row" style="margin-bottom: 10px;">
                    <div class="window" style="flex: 1;">
                        <div class="window-body">
                            <div class="field-row" style="justify-content: space-around; text-align: center;">
                                <div style="flex: 1;">
                                    <p style={ match_colors[i].0 }>{ &m.player_1_name }</p>
                                    <p>"Points: " { m.player_1_points }</p>
                                    if m.finished == 0 {
                                        <button axm-click={ Msg::AddPoint(m.id, PlayerNum::One) }>"+ point"</button>
                                    }
                                </div>
                                <div style="flex: 1;">
                                    <p style={ match_colors[i].1 }>{ &m.player_2_name }</p>
                                    <p>"Points: " { m.player_2_points }</p>
                                    if m.finished == 0 {
                                        <button axm-click={ Msg::AddPoint(m.id, PlayerNum::Two) }>"+ point"</button>
                                    }
                                </div>
                            </div>
                            <div class="field-row" style="justify-content: center; margin-top: 10px;">
                                if m.finished != 0 {
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
// Observer view
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct ObserverApp {
    state: AppState,
    matches: Vec<Match>,
}

impl ObserverApp {
    fn new(state: AppState) -> Self {
        Self {
            state,
            matches: Vec::new(),
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
        let mut rx = self.state.tx.subscribe();
        tokio::spawn(async move {
            while let Ok(RefreshPing) = rx.recv().await {
                if handle.send(ObserverMsg::Refresh).await.is_err() {
                    break;
                }
            }
        });
    }

    fn update(mut self, msg: ObserverMsg, _data: Option<EventData>) -> Updated<Self> {
        match msg {
            ObserverMsg::Refresh => {
                let db = self.state.db.clone();
                return Updated::new(self).spawn(async move {
                    match fetch_matches(&db).await {
                        Ok(matches) => ObserverMsg::MatchesList(matches),
                        Err(e) => {
                            tracing::error!("failed to fetch matches: {e}");
                            ObserverMsg::MatchesList(Vec::new())
                        }
                    }
                });
            }
            ObserverMsg::MatchesList(items) => self.matches = items,
        }
        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        let match_colors: Vec<(&str, &str)> = self
            .matches
            .iter()
            .map(|m| {
                if m.finished != 0 {
                    if m.player_1_points > m.player_2_points {
                        ("color: green; font-weight: bold;", "color: red;")
                    } else if m.player_2_points > m.player_1_points {
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
            if self.matches.is_empty() {
                <div style="text-align: center; padding: 48px 0;">
                    <p style="font-size: 14px;">"No matches in progress."</p>
                    <p style="margin-top: 8px;">
                        "Head over to the " <a href="/">"Admin page"</a> " to create one."
                    </p>
                </div>
            } else {
                for (i, m) in self.matches.iter().enumerate() {
                    <div class="window" style="margin-bottom: 12px;">
                        <div class="window-body">
                            <div class="field-row" style="justify-content: center; align-items: center;">
                                <div style="flex: 1; text-align: right; padding-right: 16px;">
                                    <p style={ match_colors[i].0 }>{ &m.player_1_name }</p>
                                </div>
                                <div class="field-row" style="align-items: center; gap: 8px;">
                                    <div class="sunken-panel" style="padding: 4px 14px; text-align: center; min-width: 44px;">
                                        <span style="font-size: 22px; font-weight: bold;">{ m.player_1_points }</span>
                                    </div>
                                    <span style="font-size: 18px;">":"</span>
                                    <div class="sunken-panel" style="padding: 4px 14px; text-align: center; min-width: 44px;">
                                        <span style="font-size: 22px; font-weight: bold;">{ m.player_2_points }</span>
                                    </div>
                                </div>
                                <div style="flex: 1; text-align: left; padding-left: 16px;">
                                    <p style={ match_colors[i].1 }>{ &m.player_2_name }</p>
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
    MatchesList(Vec<Match>),
}

// ---------------------------------------------------------------------------
// Messages & form data
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
enum Msg {
    Refresh,
    Noop,

    Player1Input,
    Player2Input,
    CreateMatch,
    AddPoint(i64, PlayerNum),
    FinishMatch(i64),

    MatchesList(Vec<Match>),
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

    /// Create an in-memory SQLite pool for tests with the schema applied.
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("failed to create test pool");
        init_db(&pool).await.expect("failed to init test db");
        pool
    }

    fn input_event(s: &str) -> Option<EventData> {
        Some(EventData::Input(event_data::Input::String(s.to_owned())))
    }

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

    // -- Admin tests ----------------------------------------------------------

    #[tokio::test]
    async fn initial_render() {
        let pool = test_pool().await;
        let (tx, _) = broadcast::channel(1024);
        let view = run_live_view(TennisApp::new(AppState { db: pool, tx }))
            .mount()
            .await;
        let html = view.render().await;

        assert!(html.contains("Tennis Matches"));
        assert!(html.contains("Create some and they will be listed here."));
    }

    #[tokio::test]
    async fn form_input_updates_player_names() {
        let pool = test_pool().await;
        let (tx, _) = broadcast::channel(1024);
        let view = run_live_view(TennisApp::new(AppState { db: pool, tx }))
            .mount()
            .await;

        view.send(Msg::Player1Input, input_event("Roger")).await;
        let (html, _) = view.send(Msg::Player2Input, input_event("Rafa")).await;
        assert!(html.contains("Roger"));
        assert!(html.contains("Rafa"));
    }

    #[tokio::test]
    async fn submit_button_enabled_only_when_form_valid() {
        let pool = test_pool().await;
        let (tx, _) = broadcast::channel(1024);
        let view = run_live_view(TennisApp::new(AppState { db: pool, tx }))
            .mount()
            .await;

        assert!(view.render().await.contains("disabled"));

        let (html, _) = view.send(Msg::Player1Input, input_event("Roger")).await;
        assert!(html.contains("disabled"));

        let (html, _) = view.send(Msg::Player2Input, input_event("Rafa")).await;
        assert!(!html.contains("disabled"));
    }

    #[tokio::test]
    async fn create_match() {
        let pool = test_pool().await;
        let (tx, _) = broadcast::channel(1024);
        let view = run_live_view(TennisApp::new(AppState { db: pool, tx }))
            .mount()
            .await;

        view.send(Msg::Player1Input, input_event("Roger")).await;
        view.send(Msg::Player2Input, input_event("Rafa")).await;
        let (html, _) = view
            .send(Msg::CreateMatch, form_submit("Roger", "Rafa"))
            .await;

        assert!(html.contains("Roger"));
        assert!(html.contains("Rafa"));
        assert!(!html.contains("Create some and they will be listed here."));
    }

    #[tokio::test]
    async fn add_point_to_player() {
        let pool = test_pool().await;
        let (tx, _) = broadcast::channel(1024);
        let view = run_live_view(TennisApp::new(AppState { db: pool, tx }))
            .mount()
            .await;

        view.send(Msg::Player1Input, input_event("Roger")).await;
        view.send(Msg::Player2Input, input_event("Rafa")).await;
        view.send(Msg::CreateMatch, form_submit("Roger", "Rafa"))
            .await;

        let (html, _) = view.send(Msg::AddPoint(1, PlayerNum::One), None).await;
        assert!(html.contains("Points: 1"));

        let (html, _) = view.send(Msg::AddPoint(1, PlayerNum::Two), None).await;
        assert!(html.contains("Points: 1"));
    }

    #[tokio::test]
    async fn finish_match() {
        let pool = test_pool().await;
        let (tx, _) = broadcast::channel(1024);
        let view = run_live_view(TennisApp::new(AppState { db: pool, tx }))
            .mount()
            .await;

        view.send(Msg::Player1Input, input_event("Roger")).await;
        view.send(Msg::Player2Input, input_event("Rafa")).await;
        view.send(Msg::CreateMatch, form_submit("Roger", "Rafa"))
            .await;

        let (html, _) = view.send(Msg::FinishMatch(1), None).await;
        assert!(html.contains("Finished"));
        assert!(!html.contains("+ point"));
    }

    #[tokio::test]
    async fn two_views_share_state() {
        let pool = test_pool().await;
        let (tx, _) = broadcast::channel(1024);
        let state = AppState { db: pool, tx };

        let h1 = run_live_view(TennisApp::new(state.clone()))
            .mount()
            .await;
        let h2 = run_live_view(TennisApp::new(state.clone()))
            .mount()
            .await;

        h1.send(Msg::Player1Input, input_event("Roger")).await;
        h1.send(Msg::Player2Input, input_event("Rafa")).await;
        h1.send(Msg::CreateMatch, form_submit("Roger", "Rafa"))
            .await;

        let (html2, _) = h2.send(Msg::Refresh, None).await;
        assert!(html2.contains("Roger"));
        assert!(html2.contains("Rafa"));

        h2.send(Msg::AddPoint(1, PlayerNum::One), None).await;
        let (html1, _) = h1.send(Msg::Refresh, None).await;
        assert!(html1.contains("Points: 1"));
    }

    // -- Observer tests -------------------------------------------------------

    #[tokio::test]
    async fn observer_shows_empty_state() {
        let pool = test_pool().await;
        let (tx, _) = broadcast::channel(1024);
        let view = run_live_view(ObserverApp::new(AppState { db: pool, tx }))
            .mount()
            .await;
        let html = view.render().await;

        assert!(html.contains("No matches in progress."));
    }

    #[tokio::test]
    async fn observer_sees_match_created_by_admin() {
        let pool = test_pool().await;
        let (tx, _) = broadcast::channel(1024);
        let state = AppState { db: pool, tx };

        let admin = run_live_view(TennisApp::new(state.clone()))
            .mount()
            .await;
        let obs = run_live_view(ObserverApp::new(state.clone()))
            .mount()
            .await;

        admin.send(Msg::Player1Input, input_event("Serena")).await;
        admin.send(Msg::Player2Input, input_event("Venus")).await;
        admin
            .send(Msg::CreateMatch, form_submit("Serena", "Venus"))
            .await;

        let (obs_html, _) = obs.send(ObserverMsg::Refresh, None).await;
        assert!(obs_html.contains("Serena"));
        assert!(obs_html.contains("Venus"));
        assert!(!obs_html.contains("button"));
    }
}
