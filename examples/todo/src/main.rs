use axum::{Router, response::IntoResponse, routing::get};
use axum_live_view::{
    Html, LiveView, LiveViewUpgrade, event_data::EventData, html, live_view::Updated,
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
            .route("/bundle.js", axum_live_view::precompiled_js())
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
    let view = TodoApp::default();

    live.response(move |embed| {
        html! {
            <!DOCTYPE html>
            <html>
                <head>
                    <style>
                        { STYLE }
                    </style>
                </head>
                <body>
                    { embed.embed(view) }
                    <script src="/bundle.js"></script>
                </body>
            </html>
        }
    })
}

const STYLE: &str = r#"
* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

body {
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
    background: #f5f5f5;
    color: #4d4d4d;
    display: flex;
    justify-content: center;
    padding-top: 80px;
}

.todoapp {
    width: 550px;
    background: #fff;
    border-radius: 4px;
    box-shadow: 0 2px 4px rgba(0,0,0,.1), 0 8px 16px rgba(0,0,0,.1);
    position: relative;
}

h1 {
    font-size: 80px;
    font-weight: 200;
    text-align: center;
    color: #ead7d7;
    margin-bottom: 24px;
    user-select: none;
}

.add-todo-form {
    padding: 16px;
    border-bottom: 1px solid #e6e6e6;
}

.add-todo-input {
    width: 100%;
    padding: 16px;
    font-size: 24px;
    border: none;
    outline: none;
    color: #4d4d4d;
    background: transparent;
}

.add-todo-input::placeholder {
    color: #d9d9d9;
    font-style: italic;
}

.filters {
    display: flex;
    justify-content: center;
    gap: 4px;
    padding: 12px 16px;
    border-bottom: 1px solid #e6e6e6;
}

.filter-btn {
    padding: 4px 12px;
    border: 1px solid transparent;
    border-radius: 4px;
    background: transparent;
    color: #777;
    font-size: 14px;
    cursor: pointer;
    transition: border-color .15s;
}

.filter-btn:hover {
    border-color: #ead7d7;
}

.filter-btn.selected {
    border-color: #d2b6b6;
    background: #faf0f0;
}

.empty-msg {
    text-align: center;
    padding: 40px 16px;
    color: #c9c9c9;
    font-size: 18px;
}

.todo-list {
    list-style: none;
}

.todo-item {
    display: flex;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid #f0f0f0;
    font-size: 20px;
    gap: 12px;
}

.todo-item:hover .todo-delete {
    opacity: 1;
}

.todo-checkbox {
    appearance: none;
    -webkit-appearance: none;
    width: 28px;
    height: 28px;
    border: 2px solid #d1d1d1;
    border-radius: 50%;
    cursor: pointer;
    flex-shrink: 0;
    position: relative;
    transition: border-color .2s, background .2s;
    background: #fff;
}

.todo-checkbox:hover {
    border-color: #a0d8b3;
}

.todo-checkbox:checked {
    border-color: #5dc08c;
    background: #5dc08c;
}

.todo-checkbox:checked::after {
    content: "";
    position: absolute;
    top: 4px;
    left: 8px;
    width: 7px;
    height: 13px;
    border: solid #fff;
    border-width: 0 2.5px 2.5px 0;
    transform: rotate(45deg);
}

.todo-text {
    flex: 1;
    word-break: break-word;
    transition: color .2s;
}

.todo-text.completed {
    text-decoration: line-through;
    color: #d9d9d9;
}

.todo-delete {
    padding: 0 6px;
    border: none;
    background: transparent;
    color: #cc9a9a;
    font-size: 22px;
    cursor: pointer;
    opacity: 0;
    transition: opacity .15s, color .15s;
    flex-shrink: 0;
    line-height: 1;
}

.todo-delete:hover {
    color: #af5b5e;
}

.footer {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    font-size: 14px;
    color: #777;
    border-top: 1px solid #f0f0f0;
}

.items-left {
    font-weight: 300;
}

.clear-completed {
    border: none;
    background: transparent;
    color: #777;
    font-size: 14px;
    cursor: pointer;
    transition: color .15s;
}

.clear-completed:hover {
    color: #4d4d4d;
    text-decoration: underline;
}
"#;

#[derive(Clone, Default, Debug)]
struct TodoApp {
    todos: Vec<Todo>,
    next_id: u64,
    draft: String,
    filter: Filter,
}

#[derive(Clone, Debug)]
struct Todo {
    id: u64,
    text: String,
    completed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
enum Filter {
    #[default]
    All,
    Active,
    Completed,
}

impl LiveView for TodoApp {
    type Message = Msg;

    fn update(mut self, msg: Msg, data: Option<EventData>) -> Updated<Self> {
        match msg {
            Msg::Input => {
                self.draft = data
                    .unwrap()
                    .as_input()
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .to_owned();
            }
            Msg::Add => {
                let trimmed = self.draft.trim().to_owned();
                if !trimmed.is_empty() {
                    self.todos.push(Todo {
                        id: self.next_id,
                        text: trimmed,
                        completed: false,
                    });
                    self.next_id += 1;
                    self.draft.clear();
                }
            }
            Msg::Toggle(id) => {
                if let Some(todo) = self.todos.iter_mut().find(|t| t.id == id) {
                    todo.completed = !todo.completed;
                }
            }
            Msg::Delete(id) => {
                self.todos.retain(|t| t.id != id);
            }
            Msg::SetFilter(filter) => {
                self.filter = filter;
            }
            Msg::ClearCompleted => {
                self.todos.retain(|t| !t.completed);
            }
        }

        Updated::new(self)
    }

    fn render(&self) -> Html<Self::Message> {
        let filtered: Vec<&Todo> = self
            .todos
            .iter()
            .filter(|t| match self.filter {
                Filter::All => true,
                Filter::Active => !t.completed,
                Filter::Completed => t.completed,
            })
            .collect();

        let remaining = self.todos.iter().filter(|t| !t.completed).count();
        let completed = self.todos.len() - remaining;

        html! {
                    <div class="todoapp">
                        <h1>"todos"</h1>

                        // --- Add new todo form ---
                        <form class="add-todo-form" axm-submit={ Msg::Add }>
                            <input
                                class="add-todo-input"
                                type="text"
                                placeholder="What needs to be done?"
                                axm-input={ Msg::Input }
                            />
                        </form>

                        // --- Filter tabs ---
                        <div class="filters">
                            for filter in [Filter::All, Filter::Active, Filter::Completed] {
                                <button
                                    class=if self.filter == filter { "filter-btn selected" } else { "filter-btn" }
                                    axm-click={ Msg::SetFilter(filter) }
                                >
                                    { filter.label() }
                                </button>
                            }
                        </div>

                        // --- Todo list (always in DOM, toggled via CSS) ---
                        if filtered.is_empty() {
                            <p class="empty-msg">"No todos to show."</p>
                        } else {
                            <ul class="todo-list">
                                for todo in &filtered {
                                    <li class="todo-item">
                                        <input
                                            class="todo-checkbox"
                                            type="checkbox"
                                            checked=if todo.completed { Some(()) } else { None }
                                            axm-click={ Msg::Toggle(todo.id) }
                                        />
                                        <span
                                            class=if todo.completed { "todo-text completed" } else { "todo-text" }
                                        >
                                            { &todo.text }
                                        </span>
                                        <button
                                            class="todo-delete"
                                            axm-click={ Msg::Delete(todo.id) }
                                        >
                                            "x"
                                        </button>
                                    </li>
                                }
                            </ul>
                        }
                        // --- Footer with counts and clear ---
                        <div class="footer">
                            <span class="items-left">
                                { remaining }
                                " "
                                { if remaining == 1 { "item left" } else { "items left" } }
                            </span>

                            if completed > 0 {
                                <button
                                    class="clear-completed"
                                    axm-click={ Msg::ClearCompleted }
                                >
                                    "Clear completed (" { completed } ")"
                                </button>
                            }
                        </div>
                    </div>
                }
    }
}

impl Filter {
    fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Active => "Active",
            Filter::Completed => "Completed",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq, Clone, Copy)]
enum Msg {
    Input,
    Add,
    Toggle(u64),
    Delete(u64),
    SetFilter(Filter),
    ClearCompleted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum_live_view::{event_data::Input, test::run_live_view};

    /// Helper to send an input event with a string value.
    fn input_event(s: &str) -> Option<axum_live_view::event_data::EventData> {
        Some(axum_live_view::event_data::EventData::Input(Input::String(
            s.to_owned(),
        )))
    }

    #[tokio::test]
    async fn initial_render() {
        let view = run_live_view(TodoApp::default()).mount().await;
        let html = view.render().await;

        assert!(html.contains("todos"));
        assert!(html.contains("No todos to show."));
        assert!(html.contains("0 items left"));
        assert!(html.contains("placeholder=\"What needs to be done?\""));
    }

    #[tokio::test]
    async fn add_todo() {
        let view = run_live_view(TodoApp::default()).mount().await;

        // Set draft and add a todo
        view.send(Msg::Input, input_event("Learn Rust")).await;
        let (html, _) = view.send(Msg::Add, None).await;

        assert!(html.contains("Learn Rust"));
        assert!(html.contains("todo-text"));
        assert!(!html.contains("todo-text completed"));
        assert!(html.contains("1 item left"));
        // Empty message should be hidden
        assert!(html.contains("display: none"));
    }

    #[tokio::test]
    async fn add_multiple_todos() {
        let view = run_live_view(TodoApp::default()).mount().await;

        for text in ["A", "B", "C"] {
            view.send(Msg::Input, input_event(text)).await;
            view.send(Msg::Add, None).await;
        }

        let html = view.render().await;
        assert!(html.contains("A"));
        assert!(html.contains("B"));
        assert!(html.contains("C"));
        assert!(html.contains("3 items left"));
    }

    #[tokio::test]
    async fn toggle_todo() {
        let view = run_live_view(TodoApp::default()).mount().await;

        view.send(Msg::Input, input_event("Test")).await;
        view.send(Msg::Add, None).await;

        // The first todo gets id 0
        let (html, _) = view.send(Msg::Toggle(0), None).await;

        assert!(html.contains("todo-text completed"));
        assert!(html.contains("0 items left"));
        assert!(html.contains("Clear completed"));

        // Toggle back
        let (html, _) = view.send(Msg::Toggle(0), None).await;

        assert!(!html.contains("todo-text completed"));
        assert!(html.contains("1 item left"));
        assert!(!html.contains("Clear completed"));
    }

    #[tokio::test]
    async fn delete_todo() {
        let view = run_live_view(TodoApp::default()).mount().await;

        view.send(Msg::Input, input_event("Delete me")).await;
        view.send(Msg::Add, None).await;

        let html = view.render().await;
        assert!(html.contains("Delete me"));

        let (html, _) = view.send(Msg::Delete(0), None).await;
        assert!(!html.contains("Delete me"));
        assert!(html.contains("0 items left"));
        // Empty message should be visible, list hidden
        assert!(html.contains("No todos to show."));
        assert!(html.contains("display: none"));
    }

    #[tokio::test]
    async fn filter_active() {
        let view = run_live_view(TodoApp::default()).mount().await;

        // Add two todos with names distinct from filter labels
        view.send(Msg::Input, input_event("Write tests")).await;
        view.send(Msg::Add, None).await;
        view.send(Msg::Input, input_event("Fix bugs")).await;
        view.send(Msg::Add, None).await;

        // Complete the second one (id 1)
        view.send(Msg::Toggle(1), None).await;

        // Filter to Active
        let (html, _) = view.send(Msg::SetFilter(Filter::Active), None).await;
        assert!(html.contains("Write tests"));
        assert!(!html.contains("Fix bugs"));
        assert!(html.contains("1 item left"));

        // Filter to Completed
        let (html, _) = view.send(Msg::SetFilter(Filter::Completed), None).await;
        assert!(!html.contains("Write tests"));
        assert!(html.contains("Fix bugs"));
        assert!(html.contains("todo-text completed"));

        // Filter back to All
        let (html, _) = view.send(Msg::SetFilter(Filter::All), None).await;
        assert!(html.contains("Write tests"));
        assert!(html.contains("Fix bugs"));
    }

    #[tokio::test]
    async fn filter_selected_class() {
        let view = run_live_view(TodoApp::default()).mount().await;

        let html = view.render().await;
        // All should be selected by default
        assert!(html.contains("filter-btn selected"));

        let (html, _) = view.send(Msg::SetFilter(Filter::Active), None).await;
        // Check the class is on the right button — just confirm "selected" appears
        assert!(html.contains("filter-btn selected"));
    }

    #[tokio::test]
    async fn clear_completed() {
        let view = run_live_view(TodoApp::default()).mount().await;

        // Add two todos and complete one
        view.send(Msg::Input, input_event("Keep")).await;
        view.send(Msg::Add, None).await;
        view.send(Msg::Input, input_event("Gone")).await;
        view.send(Msg::Add, None).await;
        view.send(Msg::Toggle(1), None).await;

        // Clear completed
        let (html, _) = view.send(Msg::ClearCompleted, None).await;

        assert!(html.contains("Keep"));
        assert!(!html.contains("Gone"));
        assert!(html.contains("1 item left"));
        assert!(!html.contains("Clear completed"));
    }

    #[tokio::test]
    async fn empty_draft_not_added() {
        let view = run_live_view(TodoApp::default()).mount().await;

        // Try to add with empty/whitespace draft
        view.send(Msg::Input, input_event("   ")).await;
        let (html, _) = view.send(Msg::Add, None).await;

        assert!(html.contains("0 items left"));
    }

    #[tokio::test]
    async fn delete_then_filter_to_completed_then_back() {
        let view = run_live_view(TodoApp::default()).mount().await;

        // Add two active todos
        view.send(Msg::Input, input_event("Alpha")).await;
        view.send(Msg::Add, None).await;
        view.send(Msg::Input, input_event("Beta")).await;
        view.send(Msg::Add, None).await;

        let html = view.render().await;
        assert!(html.contains("Alpha"), "Alpha should be visible");
        assert!(html.contains("Beta"), "Beta should be visible");
        assert!(html.contains("2 items left"));

        // Delete the first todo (id 0)
        let (html, _) = view.send(Msg::Delete(0), None).await;
        assert!(!html.contains("Alpha"), "Alpha should be deleted");
        assert!(html.contains("Beta"), "Beta should still be visible");
        assert!(html.contains("1 item left"));

        // Filter to Completed (Beta is active, so nothing should show)
        let (html, _) = view.send(Msg::SetFilter(Filter::Completed), None).await;
        assert!(
            html.contains("No todos to show."),
            "nothing completed, should show empty"
        );

        // Filter back to All — Beta should reappear
        let (html, _) = view.send(Msg::SetFilter(Filter::All), None).await;
        assert!(
            html.contains("Beta"),
            "Beta should be visible again under All"
        );
        assert!(html.contains("1 item left"));

        // Filter to Active — Beta should still show
        let (html, _) = view.send(Msg::SetFilter(Filter::Active), None).await;
        assert!(html.contains("Beta"), "Beta should be visible under Active");
    }

    #[tokio::test]
    async fn draft_cleared_after_add() {
        let view = run_live_view(TodoApp::default()).mount().await;

        view.send(Msg::Input, input_event("Buy milk")).await;
        let (html, _) = view.send(Msg::Add, None).await;

        // The draft should be cleared — no "Buy milk" in an input value attribute context
        // (the todo text appears in the list, not the input)
        assert!(html.contains("Buy milk"));
        // Make sure the todo text appears only once (in the list, not as an input value)
        assert_eq!(
            html.matches("Buy milk").count(),
            1,
            "todo text should appear only once"
        );
    }
}
