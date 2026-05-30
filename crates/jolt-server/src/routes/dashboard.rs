use axum::response::Html;

pub async fn dashboard() -> Html<&'static str> {
    Html(include_str!("../dashboard.html"))
}
