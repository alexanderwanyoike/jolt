use jolt_network::DaemonHandle;

#[derive(Clone)]
pub struct AppState {
    pub daemon: DaemonHandle,
}
