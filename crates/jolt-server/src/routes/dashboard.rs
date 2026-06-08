use axum::response::Html;

pub async fn console_entry() -> Html<&'static str> {
    Html(include_str!("../console-entry.html"))
}
