use jolt_network::DaemonHandle;

use crate::session_store::AppSessionStore;

#[derive(Clone)]
pub struct AppState {
    pub daemon: DaemonHandle,
    pub sessions: AppSessionStore,
}
