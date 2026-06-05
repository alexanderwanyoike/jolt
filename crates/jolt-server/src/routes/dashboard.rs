use axum::response::{Html, Redirect};

pub async fn console_entry() -> Html<&'static str> {
    Html(include_str!("../console-entry.html"))
}

pub async fn dashboard_redirect() -> Redirect {
    Redirect::temporary("/debug/dashboard")
}

pub async fn debug_dashboard() -> Html<&'static str> {
    Html(include_str!("../dashboard.html"))
}
