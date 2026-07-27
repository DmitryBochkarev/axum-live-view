use axum::{Router, response::IntoResponse};
use axum_live_view::{
    Html, LiveView, LiveViewUpgrade, event_data::EventData, html, live_page, live_view::Updated,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, time::{SystemTime, UNIX_EPOCH}};
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
    axum::serve(listener, app).await.unwrap();
}

async fn root(live: LiveViewUpgrade) -> impl IntoResponse {
    live.response(move |embed| async move {
        let view = Wordle::new_random();

        html! {
            <!DOCTYPE html>
            <html>
                <head>
                    <style>{ STYLE }</style>
                </head>
                <body>
                    { embed.embed(view) }
                    <script src="/_live_view.js"></script>
                </body>
            </html>
        }
    })
    .await
}

// ---------------------------------------------------------------------------
// Game state
// ---------------------------------------------------------------------------

/// A small built-in word list. In a real application you'd load a larger
/// dictionary and validate guesses against it.
#[derive(Clone, Debug)]
struct Wordle {
    target: String,
    guesses: Vec<Vec<(char, LetterStatus)>>,
    current_guess: Vec<char>,
    keyboard_status: HashMap<char, LetterStatus>,
    game_over: bool,
    won: bool,
    message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum LetterStatus {
    Correct,
    Present,
    Absent,
}

impl Wordle {
    const WORD_LEN: usize = 5;
    const MAX_GUESSES: usize = 6;

    const WORDS: &[&str] = &[
        "hello", "world", "rusty", "crate", "async", "tokio", "serde", "frame", "stack",
        "bytes", "error", "match", "macro", "trait",
    ];

    fn new_random() -> Self {
        let idx = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .subsec_nanos() as usize
            % Self::WORDS.len();
        Self::new(Self::WORDS[idx])
    }

    fn new(target: &str) -> Self {
        Self {
            target: target.to_lowercase(),
            guesses: Vec::new(),
            current_guess: Vec::new(),
            keyboard_status: HashMap::new(),
            game_over: false,
            won: false,
            message: None,
        }
    }

    /// Process a key input from either physical or on-screen keyboard.
    fn process_key(&mut self, key: &str) {
        if self.game_over {
            return;
        }

        match key {
            "Enter" => {
                if self.current_guess.len() == Wordle::WORD_LEN {
                    self.submit_guess();
                }
            }
            "Backspace" => {
                self.current_guess.pop();
            }
            s if s.len() == 1 => {
                let c = s.chars().next().unwrap();
                if c.is_ascii_alphabetic() && self.current_guess.len() < Wordle::WORD_LEN {
                    self.current_guess.push(c.to_ascii_lowercase());
                }
            }
            _ => {}
        }
    }

    fn submit_guess(&mut self) {
        let guess_chars: Vec<char> = self.current_guess.drain(..).collect();
        let guess_word: String = guess_chars.iter().collect();

        // Count remaining letters in the target (excluding exact matches)
        let mut target_remaining: Vec<char> = self
            .target
            .chars()
            .zip(guess_chars.iter())
            .filter(|(t, g)| t != *g)
            .map(|(t, _)| t)
            .collect();

        // First pass: mark correct positions
        let mut result: Vec<(char, LetterStatus)> = Vec::with_capacity(Wordle::WORD_LEN);
        for (i, &g) in guess_chars.iter().enumerate() {
            let status = if self.target.chars().nth(i) == Some(g) {
                LetterStatus::Correct
            } else if let Some(pos) = target_remaining.iter().position(|&t| t == g) {
                target_remaining.remove(pos);
                LetterStatus::Present
            } else {
                LetterStatus::Absent
            };
            result.push((g, status));
        }

        // Update keyboard: "upgrade" statuses (Correct > Present > Absent)
        for &(ch, status) in &result {
            let entry = self
                .keyboard_status
                .entry(ch)
                .or_insert(LetterStatus::Absent);
            *entry = match (*entry, status) {
                (LetterStatus::Correct, _) | (_, LetterStatus::Correct) => LetterStatus::Correct,
                (LetterStatus::Present, _) | (_, LetterStatus::Present) => LetterStatus::Present,
                _ => LetterStatus::Absent,
            };
        }

        if guess_word == self.target {
            self.game_over = true;
            self.won = true;
            self.message = Some(format!(
                "You won in {} {}! 🎉",
                self.guesses.len() + 1,
                if self.guesses.len() == 0 {
                    "guess"
                } else {
                    "guesses"
                }
            ));
        } else if self.guesses.len() + 1 >= Wordle::MAX_GUESSES {
            self.game_over = true;
            self.message = Some(format!("The word was: {}", self.target.to_uppercase()));
        }

        self.guesses.push(result);
    }

    /// Build a flat, normalized grid of all 6 rows × 5 cells.  Each cell has
    /// an optional character and an optional status.  This uniform shape lets
    /// the HTML diff safely compare any two renders.
    fn grid(&self) -> Vec<Vec<CellData>> {
        let mut rows: Vec<Vec<CellData>> = Vec::with_capacity(Wordle::MAX_GUESSES);

        // Completed guesses
        for guess in &self.guesses {
            let row = guess
                .iter()
                .map(|&(ch, status)| CellData {
                    letter: Some(ch),
                    status: Some(status),
                })
                .collect();
            rows.push(row);
        }

        // Current in-progress guess
        if rows.len() < Wordle::MAX_GUESSES {
            let row: Vec<CellData> = (0..Wordle::WORD_LEN)
                .map(|col| CellData {
                    letter: self.current_guess.get(col).copied(),
                    status: None,
                })
                .collect();
            rows.push(row);
        }

        // Empty rows
        while rows.len() < Wordle::MAX_GUESSES {
            let row = (0..Wordle::WORD_LEN)
                .map(|_| CellData {
                    letter: None,
                    status: None,
                })
                .collect();
            rows.push(row);
        }

        rows
    }

    /// CSS class for a cell.
    fn cell_class(status: Option<LetterStatus>) -> &'static str {
        match status {
            Some(LetterStatus::Correct) => "cell correct",
            Some(LetterStatus::Present) => "cell present",
            Some(LetterStatus::Absent) => "cell absent",
            None => "cell",
        }
    }

    /// CSS class for a keyboard key.
    fn key_class(&self, letter: char) -> &'static str {
        match self.keyboard_status.get(&letter) {
            Some(LetterStatus::Correct) => "key correct",
            Some(LetterStatus::Present) => "key present",
            Some(LetterStatus::Absent) => "key absent",
            _ => "key",
        }
    }
}

/// Pre-computed cell data with a uniform shape for safe diffing.
#[derive(Clone, Debug)]
struct CellData {
    letter: Option<char>,
    status: Option<LetterStatus>,
}

// ---------------------------------------------------------------------------
// LiveView impl
// ---------------------------------------------------------------------------

impl LiveView for Wordle {
    type Message = Msg;

    fn update(mut self, msg: Msg, data: Option<EventData>) -> Updated<Self> {
        match msg {
            Msg::KeyDown => {
                if let Some(key_data) = data.and_then(|d| d.as_key().cloned()) {
                    self.process_key(key_data.key());
                }
            }
            Msg::PressKey(c) => {
                self.process_key(&c.to_string());
            }
            Msg::Backspace => {
                self.process_key("Backspace");
            }
            Msg::Enter => {
                self.process_key("Enter");
            }
        }

        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        html! {
            <div class="game-container" axm-window-keydown={ Msg::KeyDown }>
                <h1>"wordle"</h1>

                // --- Message banner ---
                if let Some(msg) = &self.message {
                    <div class="message">{ msg }</div>
                }

                // --- Game board ---
                <div class="board">
                    for row in self.grid() {
                        <div class="row">
                            for cell in row {
                                <div class={ Wordle::cell_class(cell.status) }>
                                    if let Some(ch) = cell.letter {
                                        { ch.to_ascii_uppercase().to_string() }
                                    }
                                </div>
                            }
                        </div>
                    }
                </div>

                // --- On-screen keyboard ---
                <div class="keyboard">
                    // Row 1
                    <div class="keyboard-row">
                        for &ch in &['q', 'w', 'e', 'r', 't', 'y', 'u', 'i', 'o', 'p'] {
                            <button class={ self.key_class(ch) } axm-click={ Msg::PressKey(ch) }>
                                { ch.to_ascii_uppercase().to_string() }
                            </button>
                        }
                    </div>
                    // Row 2
                    <div class="keyboard-row">
                        <div class="spacer"></div>
                        for &ch in &['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'] {
                            <button class={ self.key_class(ch) } axm-click={ Msg::PressKey(ch) }>
                                { ch.to_ascii_uppercase().to_string() }
                            </button>
                        }
                        <div class="spacer"></div>
                    </div>
                    // Row 3
                    <div class="keyboard-row">
                        <button class="key wide" axm-click={ Msg::Enter }>"Enter"</button>
                        for &ch in &['z', 'x', 'c', 'v', 'b', 'n', 'm'] {
                            <button class={ self.key_class(ch) } axm-click={ Msg::PressKey(ch) }>
                                { ch.to_ascii_uppercase().to_string() }
                            </button>
                        }
                        <button class="key wide" axm-click={ Msg::Backspace }>"⌫"</button>
                    </div>
                </div>
            </div>
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone)]
enum Msg {
    KeyDown,
    PressKey(char),
    Backspace,
    Enter,
}

// ---------------------------------------------------------------------------
// CSS
// ---------------------------------------------------------------------------

const STYLE: &str = r#"
* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

body {
    font-family: 'Clear Sans', 'Helvetica Neue', Arial, sans-serif;
    background: #121213;
    color: #ffffff;
    display: flex;
    justify-content: center;
    padding-top: 40px;
    min-height: 100vh;
}

.game-container {
    max-width: 500px;
    width: 100%;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 24px;
}

h1 {
    font-size: 36px;
    font-weight: 700;
    letter-spacing: 4px;
    text-transform: uppercase;
}

.message {
    font-size: 16px;
    font-weight: 700;
    padding: 12px 24px;
    border-radius: 4px;
    background: #538d4e;
    color: #ffffff;
}

/* Board */
.board {
    display: flex;
    flex-direction: column;
    gap: 5px;
}

.row {
    display: flex;
    gap: 5px;
}

.cell {
    width: 62px;
    height: 62px;
    border: 2px solid #3a3a3c;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 32px;
    font-weight: 700;
    text-transform: uppercase;
    color: #ffffff;
}

.cell.correct {
    background: #538d4e;
    border-color: #538d4e;
}

.cell.present {
    background: #b59f3b;
    border-color: #b59f3b;
}

.cell.absent {
    background: #3a3a3c;
    border-color: #3a3a3c;
}

/* Keyboard */
.keyboard {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
}

.keyboard-row {
    display: flex;
    gap: 6px;
    justify-content: center;
}

.spacer {
    width: 22px;
}

.key {
    min-width: 43px;
    height: 58px;
    border: none;
    border-radius: 4px;
    background: #818384;
    color: #ffffff;
    font-size: 14px;
    font-weight: 700;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    user-select: none;
    text-transform: uppercase;
    font-family: inherit;
}

.key:hover {
    opacity: 0.85;
}

.key.wide {
    min-width: 65px;
    font-size: 12px;
}

.key.correct {
    background: #538d4e;
}

.key.present {
    background: #b59f3b;
}

.key.absent {
    background: #3a3a3c;
}
"#;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum_live_view::test::run_live_view;

    /// Count how many board-cell divs contain the uppercase form of `letter`.
    /// Board cells render as `<div ...>X</div>`, keyboard buttons as
    /// `<button ...>X</button>`.
    fn count_board_cell(haystack: &str, letter: char) -> usize {
        let upper = letter.to_ascii_uppercase().to_string();
        let pattern = format!(">{}</div>", upper);
        haystack.match_indices(&pattern).count()
    }

    #[tokio::test]
    async fn initial_render() {
        let view = run_live_view(Wordle::new("hello")).mount().await;
        let html = view.render().await;

        assert!(html.contains("wordle"), "should show title");
        assert!(html.contains(r#"class="cell""#), "should have empty cells");
        assert!(!html.contains("message"), "no message on initial render");
        // All 30 cells are rendered
        assert_eq!(
            html.matches("class=\"cell").count(),
            30,
            "30 cells on board"
        );
    }

    #[tokio::test]
    async fn type_a_letter() {
        let view = run_live_view(Wordle::new("hello")).mount().await;

        let (html, _) = view.send(Msg::PressKey('h'), None).await;

        // H should appear exactly once in a board cell (not just the keyboard)
        assert_eq!(
            count_board_cell(&html, 'h'),
            1,
            "H appears in one board cell"
        );
    }

    #[tokio::test]
    async fn type_word_and_enter() {
        let view = run_live_view(Wordle::new("hello")).mount().await;

        for c in ['h', 'e', 'l', 'l', 'o'] {
            view.send(Msg::PressKey(c), None).await;
        }
        let (html, _) = view.send(Msg::Enter, None).await;

        assert!(html.contains("correct"), "correct guess shows green");
        assert!(html.contains("You won"), "win message appears");
    }

    #[tokio::test]
    async fn wrong_guess_shows_colors() {
        let view = run_live_view(Wordle::new("hello")).mount().await;

        // "world" — w absent, o present, r absent, l correct, d absent
        for c in ['w', 'o', 'r', 'l', 'd'] {
            view.send(Msg::PressKey(c), None).await;
        }
        let (html, _) = view.send(Msg::Enter, None).await;

        assert!(html.contains("present"), "should have yellow cells");
        assert!(html.contains("correct"), "should have green for 'l'");
        assert!(html.contains("absent"), "should have gray cells");
        assert!(!html.contains("You won"), "should not have won");
    }

    #[tokio::test]
    async fn backspace_removes_last_char() {
        let view = run_live_view(Wordle::new("hello")).mount().await;

        view.send(Msg::PressKey('h'), None).await;
        view.send(Msg::PressKey('e'), None).await;
        let (html, _) = view.send(Msg::Backspace, None).await;

        assert_eq!(
            count_board_cell(&html, 'e'),
            0,
            "E should be removed from board"
        );
        assert_eq!(count_board_cell(&html, 'h'), 1, "H should remain");
    }

    #[tokio::test]
    async fn cannot_type_more_than_five() {
        let view = run_live_view(Wordle::new("hello")).mount().await;

        for c in ['a', 'b', 'c', 'd', 'e', 'f'] {
            view.send(Msg::PressKey(c), None).await;
        }
        let html = view.render().await;

        // Only first 5 letters (a..e) should appear on the board; 'f' is dropped.
        assert_eq!(count_board_cell(&html, 'a'), 1);
        assert_eq!(count_board_cell(&html, 'b'), 1);
        assert_eq!(count_board_cell(&html, 'c'), 1);
        assert_eq!(count_board_cell(&html, 'd'), 1);
        assert_eq!(count_board_cell(&html, 'e'), 1);
        assert_eq!(count_board_cell(&html, 'f'), 0, "f should not appear");
    }

    #[tokio::test]
    async fn game_over_blocks_input() {
        let view = run_live_view(Wordle::new("hello")).mount().await;

        // 6 wrong guesses
        for _ in 0..6 {
            for c in ['w', 'o', 'r', 'l', 'd'] {
                view.send(Msg::PressKey(c), None).await;
            }
            view.send(Msg::Enter, None).await;
        }

        let html = view.render().await;
        assert!(
            html.contains("The word was:"),
            "should show answer after all guesses used"
        );

        // Try pressing another key — should be ignored
        let (html2, _) = view.send(Msg::PressKey('a'), None).await;
        assert_eq!(html, html2, "no changes after game over");
    }

    #[tokio::test]
    async fn enter_with_incomplete_guess_is_ignored() {
        let view = run_live_view(Wordle::new("hello")).mount().await;

        view.send(Msg::PressKey('h'), None).await;
        view.send(Msg::PressKey('e'), None).await;
        let (html, _) = view.send(Msg::Enter, None).await;

        assert_eq!(count_board_cell(&html, 'h'), 1, "H still on board");
        assert_eq!(count_board_cell(&html, 'e'), 1, "E still on board");
        assert!(!html.contains("correct"), "no completed guess yet");
        assert!(!html.contains("present"));
        assert!(!html.contains("absent"));
    }

    #[tokio::test]
    async fn keyboard_updates_statuses() {
        let view = run_live_view(Wordle::new("hello")).mount().await;

        for c in ['a', 'b', 'c', 'd', 'e'] {
            view.send(Msg::PressKey(c), None).await;
        }
        let (html, _) = view.send(Msg::Enter, None).await;

        // E is present in "hello", so the E key should be yellow
        assert!(
            html.contains(r#"class="key present""#),
            "keyboard E should be marked present"
        );
    }
}
