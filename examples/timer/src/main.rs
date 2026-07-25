use axum::{
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
use std::net::SocketAddr;
use tokio::net::TcpListener;

// ---------------------------------------------------------------------------
// Server setup
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = axum_live_view::setup(
        Router::new()
            .route("/", live_page(root))

            .route("/xp.css", get(xp_css))
            .route("/ms_sans_serif.woff", get(ms_sans_serif_woff))
            .route("/ms_sans_serif.woff2", get(ms_sans_serif_woff2))
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
// Handlers
// ---------------------------------------------------------------------------

async fn root(live: LiveViewUpgrade) -> impl IntoResponse {
    let view = TimerView::default();

    live.response(move |embed| {
        html! {
            <!DOCTYPE html>
            <html>
                <head>
                    <title>"Timer"</title>
                    <link rel="stylesheet" href="/xp.css" />
                </head>
                <body style="margin: 0; padding: 0;">
                    <div class="window" style="max-width: 480px; margin: 2rem auto;">
                        <div class="title-bar">
                            <div class="title-bar-text">"⏱ Timer"</div>
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
// Timer view
// ---------------------------------------------------------------------------

/// Interval at which the timer ticks (in milliseconds).
const TICK_MS: u64 = 100;

/// Default duration in seconds.
const DEFAULT_DURATION_SECS: u64 = 10;

/// Minimum slider value (seconds).
const MIN_DURATION_SECS: u64 = 1;

/// Maximum slider value (seconds).
const MAX_DURATION_SECS: u64 = 60;

#[derive(Clone, Debug)]
struct TimerView {
    /// Elapsed time in milliseconds.
    elapsed_ms: u64,
    /// Target duration in milliseconds.
    duration_ms: u64,
}

impl Default for TimerView {
    fn default() -> Self {
        Self {
            elapsed_ms: 0,
            duration_ms: DEFAULT_DURATION_SECS * 1000,
        }
    }
}

impl LiveView for TimerView {
    type Message = Msg;

    fn mount(
        &mut self,
        _uri: Uri,
        _request_headers: &HeaderMap,
        handle: ViewHandle<Self::Message>,
    ) {
        // Spawn a background task that ticks the timer every TICK_MS.
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                std::time::Duration::from_millis(TICK_MS),
            );
            loop {
                interval.tick().await;
                if handle.send(Msg::Tick).await.is_err() {
                    return;
                }
            }
        });
    }

    fn update(mut self, msg: Msg, data: Option<EventData>) -> Updated<Self> {
        match msg {
            Msg::Tick => {
                // Only advance the timer if we haven't reached the duration yet.
                if self.elapsed_ms < self.duration_ms {
                    self.elapsed_ms = (self.elapsed_ms + TICK_MS).min(self.duration_ms);
                }
            }
            Msg::SetDuration => {
                // Extract the new duration from the slider's input event.
                if let Some(input) = data.and_then(|d| d.as_input().cloned()) {
                    if let Some(s) = input.as_str() {
                        if let Ok(secs) = s.parse::<u64>() {
                            self.duration_ms = secs * 1000;
                        }
                    }
                }
                // Note: if elapsed < duration after this update, the next Tick
                // will resume incrementing. If elapsed >= duration, the timer
                // stays stopped.
            }
            Msg::Reset => {
                self.elapsed_ms = 0;
                // Timer will resume on the next Tick since elapsed (0) < duration.
            }
        }

        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        let duration_secs = self.duration_ms / 1000;
        let elapsed_secs = self.elapsed_ms as f64 / 1000.0;
        let is_running = self.elapsed_ms < self.duration_ms;

        // Compute the fill percentage for the gauge.
        let pct = if self.duration_ms == 0 {
            100.0
        } else {
            (self.elapsed_ms as f64 / self.duration_ms as f64 * 100.0).min(100.0)
        };

        // Choose the gauge color: green while running, a muted blue when full.
        let bar_color = if is_running {
            "linear-gradient(to bottom, #348534, #3cb43c)"
        } else {
            "linear-gradient(to bottom, #3a6ea5, #5b8ec4)"
        };

        html! {
            <div class="field-row" style="margin-bottom: 12px; align-items: center;">
                <span style="font-weight: bold; min-width: 60px;">"Gauge"</span>
                <div class="sunken-panel" style="flex: 1; height: 28px; padding: 2px;">
                    <div style={
                        format!(
                            "width: {}%; height: 100%; background: {}; border-radius: 1px; transition: width 0.1s linear;",
                            pct, bar_color
                        )
                    }></div>
                </div>
            </div>

            <div class="field-row" style="margin-bottom: 12px;">
                <span style="font-weight: bold; min-width: 60px;">"Elapsed"</span>
                <div class="sunken-panel" style="flex: 1; padding: 4px 8px;">
                    <span>{ format!("{:.1} s", elapsed_secs) }</span>
                </div>
            </div>

            <div class="field-row" style="margin-bottom: 12px;">
                <span style="font-weight: bold; min-width: 60px;">"Duration"</span>
                <div class="sunken-panel" style="flex: 1; padding: 4px 8px;">
                    <span>{ format!("{} s", duration_secs) }</span>
                </div>
            </div>

            <div class="field-row" style="margin-bottom: 18px; align-items: center;">
                <span style="font-weight: bold; min-width: 60px;">"Slider"</span>
                <input
                    type="range"
                    style="flex: 1;"
                    min={ MIN_DURATION_SECS.to_string() }
                    max={ MAX_DURATION_SECS.to_string() }
                    step="1"
                    value={ duration_secs.to_string() }
                    axm-input={ Msg::SetDuration }
                />
            </div>

            <div class="field-row">
                <button axm-click={ Msg::Reset }>"Reset"</button>
                if !is_running {
                    <span style="margin-left: 12px; color: #888; font-style: italic;">
                        "Timer finished"
                    </span>
                }
            </div>
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
enum Msg {
    /// Emitted by the background timer interval.
    Tick,
    /// Slider value changed; new duration is in EventData::Input.
    SetDuration,
    /// Reset button clicked.
    Reset,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum_live_view::{event_data, test::run_live_view};

    /// Helper to build an input event with a string value (simulating a slider
    /// change).
    fn input_event(s: &str) -> Option<EventData> {
        Some(EventData::Input(event_data::Input::String(s.to_owned())))
    }

    #[tokio::test]
    async fn initial_render() {
        let view = run_live_view(TimerView::default()).mount().await;
        let html = view.render().await;

        assert!(html.contains("Gauge"));
        assert!(html.contains("0.0 s"));
        assert!(html.contains("10 s")); // default duration
        assert!(html.contains("Reset"));
        assert!(html.contains(r#"type="range""#));
        // Timer is running initially, so no "finished" text.
        assert!(!html.contains("Timer finished"));
    }

    #[tokio::test]
    async fn tick_advances_elapsed() {
        let view = run_live_view(TimerView::default()).mount().await;

        // Each tick advances by 100ms = 0.1s.
        let (html, _) = view.send(Msg::Tick, None).await;
        assert!(html.contains("0.1 s"));

        let (html, _) = view.send(Msg::Tick, None).await;
        assert!(html.contains("0.2 s"));
    }

    #[tokio::test]
    async fn tick_stops_when_elapsed_reaches_duration() {
        let mut view_state = TimerView::default();
        // Set a short duration of 300ms.
        view_state.duration_ms = 300;

        let view = run_live_view(view_state).mount().await;

        // 3 ticks = 300ms → reaches the limit.
        view.send(Msg::Tick, None).await;
        view.send(Msg::Tick, None).await;
        let (html, _) = view.send(Msg::Tick, None).await;

        assert!(html.contains("0.3 s"));
        assert!(html.contains("Timer finished"));

        // Another tick should not change the elapsed time.
        let (html, _) = view.send(Msg::Tick, None).await;
        assert!(html.contains("0.3 s"));
    }

    #[tokio::test]
    async fn tick_clamps_to_duration() {
        let mut view_state = TimerView::default();
        // 300ms with 100ms ticks — after 3 ticks it should clamp to exactly 300ms.
        view_state.duration_ms = 300;

        let view = run_live_view(view_state).mount().await;

        view.send(Msg::Tick, None).await; // 100ms
        view.send(Msg::Tick, None).await; // 200ms
        let (html, _) = view.send(Msg::Tick, None).await; // clamps to 300ms

        assert!(html.contains("0.3 s"));
    }

    #[tokio::test]
    async fn set_duration_updates_immediately() {
        let view = run_live_view(TimerView::default()).mount().await;

        let (html, _) = view
            .send(Msg::SetDuration, input_event("20"))
            .await;

        assert!(html.contains("20 s"));
    }

    #[tokio::test]
    async fn increasing_duration_restarts_timer() {
        let mut view_state = TimerView::default();
        view_state.duration_ms = 300;

        let view = run_live_view(view_state).mount().await;

        // Run to completion.
        view.send(Msg::Tick, None).await;
        view.send(Msg::Tick, None).await;
        let (html, _) = view.send(Msg::Tick, None).await;
        assert!(html.contains("Timer finished"));
        assert!(html.contains("0.3 s"));

        // Increase the duration from 300ms to 10s.
        let (html, _) = view
            .send(Msg::SetDuration, input_event("10"))
            .await;
        assert!(!html.contains("Timer finished"));
        assert!(html.contains("10 s"));

        // Timer should tick again.
        let (html, _) = view.send(Msg::Tick, None).await;
        assert!(html.contains("0.4 s"));
    }

    #[tokio::test]
    async fn decreasing_duration_below_elapsed_stops_timer() {
        let view = run_live_view(TimerView::default()).mount().await;

        // Advance to 2s.
        for _ in 0..20 {
            view.send(Msg::Tick, None).await;
        }
        let (html, _) = view.send(Msg::Tick, None).await; // 2.1s
        assert!(!html.contains("Timer finished"));

        // Drop duration to 1s. Since 2.1s >= 1s, timer should stop.
        let (html, _) = view
            .send(Msg::SetDuration, input_event("1"))
            .await;
        assert!(html.contains("Timer finished"));
        assert!(html.contains("1 s"));

        // Another tick should not advance elapsed.
        let (html, _) = view.send(Msg::Tick, None).await;
        assert!(html.contains("2.1 s"));
    }

    #[tokio::test]
    async fn reset_sets_elapsed_to_zero() {
        let view = run_live_view(TimerView::default()).mount().await;

        // Advance several ticks.
        for _ in 0..15 {
            view.send(Msg::Tick, None).await;
        }
        let (html, _) = view.send(Msg::Tick, None).await; // 1.6s
        assert!(html.contains("1.6 s"));

        // Reset.
        let (html, _) = view.send(Msg::Reset, None).await;
        assert!(html.contains("0.0 s"));
        assert!(!html.contains("Timer finished"));

        // Timer should resume ticking.
        let (html, _) = view.send(Msg::Tick, None).await;
        assert!(html.contains("0.1 s"));
    }

    #[tokio::test]
    async fn reset_restarts_completed_timer() {
        let mut view_state = TimerView::default();
        view_state.duration_ms = 500;

        let view = run_live_view(view_state).mount().await;

        // Run to completion.
        for _ in 0..5 {
            view.send(Msg::Tick, None).await;
        }
        let (html, _) = view.send(Msg::Tick, None).await; // 0.5s
        assert!(html.contains("Timer finished"));

        // Reset.
        let (html, _) = view.send(Msg::Reset, None).await;
        assert!(html.contains("0.0 s"));
        assert!(!html.contains("Timer finished"));

        // Timer restarts.
        let (html, _) = view.send(Msg::Tick, None).await;
        assert!(html.contains("0.1 s"));
    }

    #[tokio::test]
    async fn gauge_shows_correct_fill() {
        let mut view_state = TimerView::default();
        view_state.duration_ms = 1000; // 1s

        let view = run_live_view(view_state).mount().await;

        // At 500ms, gauge should be at 50%.
        for _ in 0..5 {
            view.send(Msg::Tick, None).await;
        }
        let html = view.render().await;
        assert!(html.contains("width: 50%"));

        // After 10 total ticks (1s), gauge should be full.
        for _ in 0..5 {
            view.send(Msg::Tick, None).await;
        }
        let html = view.render().await;
        assert!(html.contains("width: 100%"));
    }

    #[tokio::test]
    async fn slider_adjustment_updates_duration_only_not_elapsed() {
        let view = run_live_view(TimerView::default()).mount().await;

        // Advance to 3s (30 ticks × 100ms).
        for _ in 0..30 {
            view.send(Msg::Tick, None).await;
        }
        let html = view.render().await;
        assert!(html.contains("3.0 s"));

        // Change duration.
        let (html, _) = view
            .send(Msg::SetDuration, input_event("5"))
            .await;
        assert!(html.contains("5 s"));
        assert!(html.contains("3.0 s")); // elapsed unchanged
    }

    #[tokio::test]
    async fn zero_duration_stops_immediately() {
        let view = run_live_view(TimerView::default()).mount().await;

        let (html, _) = view
            .send(Msg::SetDuration, input_event("0"))
            .await;
        assert!(html.contains("Timer finished"));
    }
}
