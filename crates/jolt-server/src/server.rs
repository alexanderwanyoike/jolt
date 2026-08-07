use axum::extract::{ConnectInfo, Request};
use axum::http::{header, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::{delete, get, post};
use axum::Router;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

use jolt_network::DaemonHandle;

use crate::device_authority::DeviceAuthorityStore;
use crate::identity_recovery::IdentityRecoveryStore;
use crate::local_identities::LocalIdentityStore;
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
    let identity_recovery = IdentityRecoveryStore::open_default();
    build_router_with_stores(daemon, sessions, network_settings, identity_recovery)
}

pub fn build_router_with_stores(
    daemon: DaemonHandle,
    sessions: AppSessionStore,
    network_settings: NetworkSettingsStore,
    identity_recovery: IdentityRecoveryStore,
) -> Router {
    let local_identities = LocalIdentityStore::open(
        daemon.local_identity_address().map(ToOwned::to_owned),
        identity_recovery.local_identities_dir(),
    )
    .unwrap_or_else(|err| panic!("failed to open local identity store: {err}"));
    let device_authority = DeviceAuthorityStore::new();
    let state = AppState {
        daemon,
        sessions,
        network_settings,
        local_identities,
        device_authority,
        identity_recovery,
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
        .route("/app/v1/append", post(routes::app_api::append_record))
        .route(
            "/app/v1/enumerate",
            post(routes::app_api::enumerate_append_records),
        )
        .route(
            "/app/v1/encrypted/publish",
            post(routes::app_api::publish_encrypted),
        )
        .route(
            "/app/v1/encrypted/append",
            post(routes::app_api::append_encrypted),
        )
        .route(
            "/app/v1/encrypted/decrypt",
            post(routes::app_api::decrypt_encrypted),
        )
        .route(
            "/app/v1/encrypted/open",
            post(routes::app_api::open_encrypted),
        )
        .route(
            "/app/v1/encrypted/rewrap",
            post(routes::app_api::rewrap_encrypted),
        )
        .route("/app/v1/published", get(routes::app_api::list_published))
        .route(
            "/app/v1/ingress/pending",
            get(routes::ingress::list_pending_ingress),
        )
        .route(
            "/app/v1/ingress/send",
            post(routes::ingress::send_ingress_by_identity),
        )
        .route(
            "/app/v1/ingress/{ingress_id}/accept",
            post(routes::ingress::accept_ingress),
        )
        .route(
            "/app/v1/ingress/{ingress_id}/open",
            post(routes::ingress::open_ingress),
        )
        .route(
            "/app/v1/ingress/{ingress_id}/reject",
            post(routes::ingress::reject_ingress),
        )
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
            "/admin/v1/identities",
            get(routes::local_identities::list).post(routes::local_identities::create),
        )
        .route(
            "/admin/v1/identities/{identity}",
            delete(routes::local_identities::delete_identity),
        )
        .route(
            "/admin/v1/identities/export",
            post(routes::identity_recovery::export_identity),
        )
        .route(
            "/admin/v1/identities/import",
            post(routes::identity_recovery::import_identity),
        )
        .route(
            "/admin/v1/identities/active",
            post(routes::local_identities::select_active),
        )
        .route(
            "/admin/v1/device-authority",
            get(routes::device_authority::list),
        )
        .route(
            "/admin/v1/device-authority/devices",
            post(routes::device_authority::authorize_device),
        )
        .route(
            "/admin/v1/device-authority/devices/{device_id}/revoke",
            post(routes::device_authority::revoke_device),
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
        .route(
            "/admin/v1/relay/status",
            get(routes::relay_status::get_status),
        )
        .route(
            "/admin/v1/relay/diagnose/identity",
            post(routes::relay_diagnose::diagnose_identity),
        )
        .route(
            "/admin/v1/reachability",
            post(routes::reachability::publish_local_reachability),
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
            "/api/v1/identities/{identity}/reachability",
            get(routes::reachability::get_identity_reachability),
        )
        .route(
            "/api/v1/ingress",
            post(routes::ingress::submit_ingress)
                .options(routes::ingress::ingress_preflight)
                .layer(public_ingress_cors()),
        )
        .route(
            "/api/v1/home-relay/availability",
            get(routes::home_relay::availability),
        )
        .route("/api/v1/home-relay/pins", post(routes::home_relay::pin))
        .route(
            "/api/v1/relay/capabilities",
            get(routes::relay::capabilities),
        )
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
        .layer(middleware::from_fn(restrict_remote_admin_requests))
        .with_state(state)
}

async fn restrict_remote_admin_requests(
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if request.uri().path().starts_with("/admin/") && !peer.ip().is_loopback() {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(request).await)
}

fn public_ingress_cors() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE])
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
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
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
    let app = build_router_with_stores(
        daemon,
        sessions,
        network_settings,
        IdentityRecoveryStore::open_default(),
    );
    start_server_with_router(app, port).await
}

pub async fn start_server_with_explicit_stores(
    daemon: DaemonHandle,
    port: u16,
    sessions: AppSessionStore,
    network_settings: NetworkSettingsStore,
    identity_recovery: IdentityRecoveryStore,
) -> Result<(u16, tokio::task::JoinHandle<()>), Box<dyn std::error::Error + Send + Sync>> {
    let app = build_router_with_stores(daemon, sessions, network_settings, identity_recovery);
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
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .ok();
    });

    Ok((actual_port, handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tower::ServiceExt;

    #[tokio::test]
    async fn admin_routes_reject_non_loopback_clients() {
        let app = Router::new()
            .route("/admin/v1/secret", get(|| async { "secret" }))
            .route("/api/v1/public", get(|| async { "public" }))
            .layer(axum::middleware::from_fn(restrict_remote_admin_requests));
        let remote = ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 0, 2, 10)),
            4242,
        ));

        let mut admin_request = Request::builder()
            .uri("/admin/v1/secret")
            .body(Body::empty())
            .unwrap();
        admin_request.extensions_mut().insert(remote);
        let admin_response = app.clone().oneshot(admin_request).await.unwrap();
        assert_eq!(admin_response.status(), StatusCode::FORBIDDEN);

        let mut public_request = Request::builder()
            .uri("/api/v1/public")
            .body(Body::empty())
            .unwrap();
        public_request.extensions_mut().insert(remote);
        let public_response = app.oneshot(public_request).await.unwrap();
        assert_eq!(public_response.status(), StatusCode::OK);
    }
}
