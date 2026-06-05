import { useEffect, useState } from "react";
import { SectionPanel } from "../components/primitives";
import {
  addBootstrapRelay,
  clearHomeRelay,
  type DaemonClient,
  loadNetworkSettings,
  removeBootstrapRelay,
  setHomeRelay,
  tauriDaemonClient
} from "../daemon/client";
import {
  tauriDaemonLifecycleClient,
  type DaemonLifecycleClient,
  type DaemonLifecycleState
} from "../daemon/lifecycle";
import type { DaemonStatus, NetworkSettingsPayload } from "../daemon/types";

type SettingsPageProps = {
  lifecycleClient?: DaemonLifecycleClient;
  daemonClient?: DaemonClient;
};

export function SettingsPage({
  lifecycleClient = tauriDaemonLifecycleClient,
  daemonClient = tauriDaemonClient
}: SettingsPageProps) {
  const [state, setState] = useState<DaemonLifecycleState | null>(null);
  const [loading, setLoading] = useState(true);
  const [action, setAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [networkSettings, setNetworkSettings] = useState<NetworkSettingsPayload | null>(null);
  const [networkStatus, setNetworkStatus] = useState<DaemonStatus | null>(null);
  const [networkLoading, setNetworkLoading] = useState(true);
  const [networkAction, setNetworkAction] = useState<string | null>(null);
  const [networkError, setNetworkError] = useState<string | null>(null);
  const [bootstrapRelayMultiaddr, setBootstrapRelayMultiaddr] = useState("");
  const [homeRelayMultiaddr, setHomeRelayMultiaddr] = useState("");
  const [homeRelayApiUrl, setHomeRelayApiUrl] = useState("");
  const [homeRelayCapability, setHomeRelayCapability] = useState("pinning");

  async function refreshLifecycle() {
    setError(null);
    setLoading(true);
    try {
      setState(await lifecycleClient.status());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }

  async function runAction(label: string, operation: () => Promise<DaemonLifecycleState>) {
    setAction(label);
    setError(null);
    try {
      setState(await operation());
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setAction(null);
    }
  }

  async function refreshNetworkSettings() {
    setNetworkError(null);
    setNetworkLoading(true);
    try {
      setNetworkSettings(await loadNetworkSettings(daemonClient));
      try {
        setNetworkStatus(await daemonClient.get<DaemonStatus>("/api/v1/status"));
      } catch {
        setNetworkStatus(null);
      }
    } catch (err) {
      setNetworkError(err instanceof Error ? err.message : String(err));
    } finally {
      setNetworkLoading(false);
    }
  }

  async function runNetworkAction(
    label: string,
    operation: () => Promise<NetworkSettingsPayload>
  ) {
    setNetworkAction(label);
    setNetworkError(null);
    try {
      setNetworkSettings(await operation());
    } catch (err) {
      setNetworkError(err instanceof Error ? err.message : String(err));
    } finally {
      setNetworkAction(null);
    }
  }

  useEffect(() => {
    void refreshLifecycle();
  }, [lifecycleClient]);

  useEffect(() => {
    void refreshNetworkSettings();
  }, [daemonClient]);

  const canStart = state?.reachability === "unavailable";
  const canControl = state?.ownership === "console";
  const busy = loading || action !== null;
  const networkBusy = networkLoading || networkAction !== null;

  return (
    <SectionPanel eyebrow="Settings" summary="daemon lifecycle and local configuration" hero>
      <div className="settings-stack">
        <div className="lifecycle-panel">
          <div className="lifecycle-header">
            <div>
              <span className="eyebrow">Daemon lifecycle</span>
              <h2>{state ? lifecycleTitle(state) : "Checking daemon"}</h2>
            </div>
            <span className={`status-pill ${state?.reachability === "healthy" ? "ok" : "pending"}`}>
              {state?.reachability ?? "checking"}
            </span>
          </div>

          {state ? (
            <div className="lifecycle-details">
              <div className="detail-row">
                <span>Daemon URL</span>
                <strong className="mono">{state.daemon_url}</strong>
              </div>
              <div className="detail-row">
                <span>Ownership</span>
                <strong className="mono">{ownershipLabel(state.ownership)}</strong>
              </div>
              <div className="detail-row">
                <span>PID</span>
                <strong className="mono">{state.pid ?? "--"}</strong>
              </div>
            </div>
          ) : null}

          {state ? <p className="lifecycle-message">{state.message}</p> : null}
          {state?.ownership === "external" ? (
            <p className="lifecycle-warning">
              Console will not stop or restart it because another process owns this daemon.
            </p>
          ) : null}
          {state?.last_error ? (
            <div className="permission-error">Daemon lifecycle error: {state.last_error}</div>
          ) : null}
          {error ? <div className="permission-error">Daemon lifecycle error: {error}</div> : null}

          <div className="lifecycle-actions">
            <button type="button" onClick={() => void refreshLifecycle()} disabled={busy}>
              Refresh lifecycle
            </button>
            <button
              type="button"
              onClick={() => void runAction("start", lifecycleClient.start)}
              disabled={busy || !canStart}
            >
              Start daemon
            </button>
            <button
              type="button"
              onClick={() => void runAction("restart", lifecycleClient.restart)}
              disabled={busy || !canControl}
            >
              Restart daemon
            </button>
            <button
              type="button"
              onClick={() => void runAction("stop", lifecycleClient.stop)}
              disabled={busy || !canControl}
            >
              Stop daemon
            </button>
          </div>

          {state?.log_tail?.length ? (
            <pre className="lifecycle-log">{state.log_tail.join("\n")}</pre>
          ) : null}
        </div>

        <div className="lifecycle-panel">
          <div className="lifecycle-header">
            <div>
              <span className="eyebrow">Network settings</span>
              <h2>Bootstrap and home relay</h2>
            </div>
            <span className={`status-pill ${networkSettings ? "ok" : "pending"}`}>
              {networkLoading ? "loading" : "admin"}
            </span>
          </div>

          {networkError ? (
            <div className="permission-error">Network settings error: {networkError}</div>
          ) : null}

          <div className="network-settings-grid">
            <section className="network-settings-card">
              <div className="network-settings-card-header">
                <h3>Configured bootstrap relays</h3>
                <span>{networkSettings?.configured_bootstrap_relay_count ?? 0}</span>
              </div>
              <RelayList
                relays={networkSettings?.configured_bootstrap_relays ?? []}
                empty="No configured bootstrap relays."
                onRemove={(multiaddr) =>
                  void runNetworkAction("remove bootstrap relay", () =>
                    removeBootstrapRelay(daemonClient, multiaddr)
                  )
                }
                disabled={networkBusy}
              />
              <div className="settings-form-row">
                <label>
                  <span>Bootstrap relay multiaddr</span>
                  <input
                    value={bootstrapRelayMultiaddr}
                    onChange={(event) => setBootstrapRelayMultiaddr(event.target.value)}
                    placeholder="/ip4/127.0.0.1/tcp/4001/p2p/12D3..."
                  />
                </label>
                <button
                  type="button"
                  disabled={networkBusy || bootstrapRelayMultiaddr.trim() === ""}
                  onClick={() =>
                    void runNetworkAction("add bootstrap relay", () =>
                      addBootstrapRelay(daemonClient, bootstrapRelayMultiaddr.trim())
                    )
                  }
                >
                  Add bootstrap relay
                </button>
              </div>
            </section>

            <section className="network-settings-card">
              <div className="network-settings-card-header">
                <h3>Built-in defaults</h3>
                <span>{networkSettings?.built_in_bootstrap_relay_count ?? 0}</span>
              </div>
              <RelayList
                relays={networkSettings?.built_in_bootstrap_relays ?? []}
                empty="No built-in bootstrap relays."
              />
              <div className="network-settings-card-header compact">
                <h3>Effective at startup</h3>
                <span>{networkSettings?.effective_bootstrap_relay_count ?? 0}</span>
              </div>
              <p className="settings-help">{effectiveBootstrapSummary(networkSettings)}</p>
            </section>

            <section className="network-settings-card">
              <div className="network-settings-card-header">
                <h3>Bootstrap health</h3>
                <span>{networkStatus?.bootstrap_state ?? "unknown"}</span>
              </div>
              <div className="network-health-grid">
                <div>
                  <span>Connected bootstrap peers</span>
                  <strong>{networkStatus?.connected_bootstrap_peers ?? "--"}</strong>
                </div>
                <div>
                  <span>Learned relay count</span>
                  <strong>{networkStatus?.known_relay_count ?? "--"}</strong>
                </div>
              </div>
              <p className="settings-help">
                Learned relays are runtime discovery state, separate from saved bootstrap settings.
              </p>
            </section>

            <section className="network-settings-card wide">
              <div className="network-settings-card-header">
                <h3>Home relay</h3>
                <span>{networkSettings?.home_relay ? "configured" : "unset"}</span>
              </div>
              {networkSettings?.home_relay ? (
                <div className="home-relay-summary">
                  <div className="detail-row">
                    <span>Peer</span>
                    <strong className="mono">{networkSettings.home_relay.peer_id ?? "--"}</strong>
                  </div>
                  <div className="detail-row">
                    <span>Multiaddr</span>
                    <strong className="mono">{networkSettings.home_relay.multiaddr}</strong>
                  </div>
                  <div className="detail-row">
                    <span>API URL</span>
                    <strong className="mono">{networkSettings.home_relay.api_url ?? "--"}</strong>
                  </div>
                </div>
              ) : (
                <p className="settings-help">No home relay is configured.</p>
              )}
              <div className="settings-form-grid">
                <label>
                  <span>Home relay multiaddr</span>
                  <input
                    value={homeRelayMultiaddr}
                    onChange={(event) => setHomeRelayMultiaddr(event.target.value)}
                    placeholder="/ip4/127.0.0.1/tcp/4001/p2p/12D3..."
                  />
                </label>
                <label>
                  <span>Home relay API URL</span>
                  <input
                    value={homeRelayApiUrl}
                    onChange={(event) => setHomeRelayApiUrl(event.target.value)}
                    placeholder="http://127.0.0.1:9870"
                  />
                </label>
                <label>
                  <span>Home relay capability</span>
                  <select
                    value={homeRelayCapability}
                    onChange={(event) => setHomeRelayCapability(event.target.value)}
                  >
                    <option value="pinning">pinning</option>
                    <option value="discovery_only">discovery only</option>
                    <option value="unknown">unknown</option>
                  </select>
                </label>
              </div>
              <div className="lifecycle-actions">
                <button
                  type="button"
                  disabled={networkBusy || homeRelayMultiaddr.trim() === ""}
                  onClick={() =>
                    void runNetworkAction("set home relay", () =>
                      setHomeRelay(daemonClient, {
                        multiaddr: homeRelayMultiaddr.trim(),
                        capability: homeRelayCapability,
                        api_url: homeRelayApiUrl.trim() || null
                      })
                    )
                  }
                >
                  Set home relay
                </button>
                <button
                  type="button"
                  disabled={networkBusy || !networkSettings?.home_relay}
                  onClick={() =>
                    void runNetworkAction("clear home relay", () => clearHomeRelay(daemonClient))
                  }
                >
                  Clear home relay
                </button>
              </div>
            </section>
          </div>
        </div>
      </div>
    </SectionPanel>
  );
}

function RelayList({
  relays,
  empty,
  onRemove,
  disabled = false
}: {
  relays: string[];
  empty: string;
  onRemove?: (multiaddr: string) => void;
  disabled?: boolean;
}) {
  if (relays.length === 0) {
    return <p className="settings-help">{empty}</p>;
  }

  return (
    <ul className="settings-relay-list">
      {relays.map((relay) => (
        <li key={relay}>
          <code>{relay}</code>
          {onRemove ? (
            <button type="button" onClick={() => onRemove(relay)} disabled={disabled}>
              Remove bootstrap relay
            </button>
          ) : null}
        </li>
      ))}
    </ul>
  );
}

function effectiveBootstrapSummary(settings: NetworkSettingsPayload | null) {
  if (!settings) return "Network settings have not loaded yet.";
  if (settings.effective_bootstrap_relay_count === 0) return "No bootstrap relays are effective.";
  if (settings.configured_bootstrap_relay_count > 0) {
    return "Configured relays take precedence over built-in defaults.";
  }
  if (settings.use_builtin_bootstrap_relays) {
    return "Built-in defaults are effective because no configured relays are set.";
  }
  return "Built-in defaults are disabled.";
}

function lifecycleTitle(state: DaemonLifecycleState) {
  if (state.ownership === "console") return "Console-owned daemon";
  if (state.ownership === "external") return "Externally-owned daemon";
  if (state.reachability === "unhealthy") return "Daemon unhealthy";
  return "Daemon unavailable";
}

function ownershipLabel(ownership: DaemonLifecycleState["ownership"]) {
  if (ownership === "console") return "Console-owned";
  if (ownership === "external") return "External";
  return "None";
}
