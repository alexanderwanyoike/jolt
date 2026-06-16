use jolt_network::DaemonHandle;

use crate::local_identities::LocalIdentityStore;
use crate::network_settings::NetworkSettingsStore;
use crate::session_store::AppSessionStore;

#[derive(Clone)]
pub struct AppState {
    pub daemon: DaemonHandle,
    pub sessions: AppSessionStore,
    pub network_settings: NetworkSettingsStore,
    pub local_identities: LocalIdentityStore,
}
