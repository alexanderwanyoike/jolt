use axum::routing::{delete, get, post};
use axum::Router;
use tokio::net::TcpListener;
use tracing::info;

use jolt_network::DaemonHandle;

use crate::network_settings::NetworkSettingsStore;
use crate::routes;
use crate::session_store::AppSessionStore;
use crate::state::AppState;

/// Build the axum router with all API routes.
pub fn build_router(daemon: DaemonHandle) -> Router {
    let sessions = AppSessionStore::open_default()
        .unwrap_or_else(|err| panic!("failed to open app session store: {err}"));
    build_router_with_session_store(daemon, sessions)
}

/// Build the axum router with an explicit app session store.
pub fn build_router_with_session_store(daemon: DaemonHandle, sessions: AppSessionStore) -> Router {
    let network_settings = NetworkSettingsStore::open_default()
        .unwrap_or_else(|err| panic!("failed to open network settings store: {err}"));
    build_router_with_stores(daemon, sessions, network_settings)
}

pub fn build_router_with_stores(
    daemon: DaemonHandle,
    sessions: AppSessionStore,
    network_settings: NetworkSettingsStore,
) -> Router {
    let state = AppState {
        daemon,
        sessions,
        network_settings,
    };

    Router::new()
        .route("/", get(routes::dashboard::console_entry))
        .route(
            "/app/v1/sessions/request",
            post(routes::app_sessions::request_session),
        )
        .route(
            "/app/v1/sessions/{request_id}",
            get(routes::app_sessions::get_request_status),
        )
        .route(
            "/app/v1/session",
            get(routes::app_sessions::current_session),
        )
        .route("/app/v1/resolve", post(routes::app_api::resolve_address))
        .route("/app/v1/fetch", post(routes::app_api::fetch_content))
        .route("/app/v1/publish", post(routes::app_api::publish_file))
        .route(
            "/app/v1/encrypted/publish",
            post(routes::app_api::publish_encrypted),
        )
        .route(
            "/app/v1/encrypted/decrypt",
            post(routes::app_api::decrypt_encrypted),
        )
        .route(
            "/app/v1/encrypted/open",
            post(routes::app_api::open_encrypted),
        )
        .route("/app/v1/published", get(routes::app_api::list_published))
        .route(
            "/app/v1/home-relay/pins",
            post(routes::app_api::pin_home_relay),
        )
        .route(
            "/admin/v1/app-requests",
            get(routes::app_sessions::list_requests),
        )
        .route(
            "/admin/v1/app-requests/{request_id}/approve",
            post(routes::app_sessions::approve_request),
        )
        .route(
            "/admin/v1/app-requests/{request_id}/reject",
            post(routes::app_sessions::reject_request),
        )
        .route(
            "/admin/v1/app-sessions",
            get(routes::app_sessions::list_sessions),
        )
        .route(
            "/admin/v1/app-sessions/{session_id}/revoke",
            post(routes::app_sessions::revoke_session),
        )
        .route(
            "/admin/v1/network-settings",
            get(routes::network_settings::get_settings),
        )
        .route(
            "/admin/v1/bootstrap-relays",
            post(routes::network_settings::add_bootstrap_relay),
        )
        .route(
            "/admin/v1/bootstrap-relays/remove",
            post(routes::network_settings::remove_bootstrap_relay),
        )
        .route(
            "/admin/v1/home-relay",
            post(routes::network_settings::set_home_relay),
        )
        .route(
            "/admin/v1/home-relay/clear",
            post(routes::network_settings::clear_home_relay),
        )
        .route("/api/v1/health", get(routes::health::health))
        .route("/api/v1/status", get(routes::status::get_status))
        .route("/api/v1/peers", get(routes::peers::list_peers))
        .route("/api/v1/peers/connect", post(routes::peers::connect_peer))
        .route("/api/v1/publish", post(routes::publish::publish_file))
        .route("/api/v1/published", get(routes::published::list))
        .route("/api/v1/fetch", post(routes::fetch::fetch_content))
        .route("/api/v1/resolve", post(routes::resolve::resolve_address))
        .route(
            "/api/v1/identities/{identity}/encryption-keys",
            get(routes::identity_encryption_keys::get_identity_encryption_keys),
        )
        .route(
            "/api/v1/home-relay/availability",
            get(routes::home_relay::availability),
        )
        .route("/api/v1/home-relay/pins", post(routes::home_relay::pin))
        .route("/api/v1/relay/pins", post(routes::relay::create_pin))
        .route(
            "/api/v1/relay/pins/{content_id}",
            get(routes::relay::pin_status),
        )
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
    start_server_with_router(app, port).await
}

/// Start the server with an explicit app session store and return the actual bound port.
pub async fn start_server_with_session_store(
    daemon: DaemonHandle,
    port: u16,
    sessions: AppSessionStore,
) -> Result<(u16, tokio::task::JoinHandle<()>), Box<dyn std::error::Error + Send + Sync>> {
    let app = build_router_with_session_store(daemon, sessions);
    start_server_with_router(app, port).await
}

pub async fn start_server_with_session_store_and_network_settings(
    daemon: DaemonHandle,
    port: u16,
    sessions: AppSessionStore,
    network_settings: NetworkSettingsStore,
) -> Result<(u16, tokio::task::JoinHandle<()>), Box<dyn std::error::Error + Send + Sync>> {
    let app = build_router_with_stores(daemon, sessions, network_settings);
    start_server_with_router(app, port).await
}

async fn start_server_with_router(
    app: Router,
    port: u16,
) -> Result<(u16, tokio::task::JoinHandle<()>), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    let addr = listener.local_addr()?;
    let actual_port = addr.port();
    info!("HTTP API listening on http://{addr}");

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });

    Ok((actual_port, handle))
}
