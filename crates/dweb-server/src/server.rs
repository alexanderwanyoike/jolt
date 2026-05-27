use axum::routing::{delete, get, post};
use axum::Router;
use tokio::net::TcpListener;
use tracing::info;

use dweb_network::DaemonHandle;

use crate::routes;
use crate::state::AppState;

/// Build the axum router with all API routes.
pub fn build_router(daemon: DaemonHandle) -> Router {
    let state = AppState { daemon };

    Router::new()
        .route("/", get(routes::dashboard::dashboard))
        .route("/dashboard", get(routes::dashboard::dashboard))
        .route("/api/v1/health", get(routes::health::health))
        .route("/api/v1/status", get(routes::status::get_status))
        .route("/api/v1/peers", get(routes::peers::list_peers))
        .route("/api/v1/publish", post(routes::publish::publish_file))
        .route("/api/v1/fetch", post(routes::fetch::fetch_content))
        .route("/api/v1/cache/stats", get(routes::cache::stats))
        .route("/api/v1/cache/entries", get(routes::cache::list_entries))
        .route("/api/v1/cache/pin/{content_id}", post(routes::cache::pin))
        .route(
            "/api/v1/cache/pin/{content_id}",
            delete(routes::cache::unpin),
        )
        .with_state(state)
}

/// Start the HTTP API server.
/// `bind_addr` controls the bind address (e.g., "127.0.0.1" for localhost-only,
/// "0.0.0.0" for all interfaces).
pub async fn start_server(
    daemon: DaemonHandle,
    port: u16,
    bind_addr: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = build_router(daemon);
    let listener = TcpListener::bind((bind_addr, port)).await?;
    let addr = listener.local_addr()?;
    info!("HTTP API listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Start the server and return the actual bound port (useful for tests with port 0).
pub async fn start_server_with_addr(
    daemon: DaemonHandle,
    port: u16,
) -> Result<(u16, tokio::task::JoinHandle<()>), Box<dyn std::error::Error + Send + Sync>> {
    let app = build_router(daemon);
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let addr = listener.local_addr()?;
    let actual_port = addr.port();
    info!("HTTP API listening on http://{addr}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    Ok((actual_port, handle))
}
