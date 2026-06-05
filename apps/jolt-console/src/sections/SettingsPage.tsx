import { useEffect, useState } from "react";
import { SectionPanel } from "../components/primitives";
import {
  tauriDaemonLifecycleClient,
  type DaemonLifecycleClient,
  type DaemonLifecycleState
} from "../daemon/lifecycle";

type SettingsPageProps = {
  lifecycleClient?: DaemonLifecycleClient;
};

export function SettingsPage({
  lifecycleClient = tauriDaemonLifecycleClient
}: SettingsPageProps) {
  const [state, setState] = useState<DaemonLifecycleState | null>(null);
  const [loading, setLoading] = useState(true);
  const [action, setAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

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

  useEffect(() => {
    void refreshLifecycle();
  }, [lifecycleClient]);

  const canStart = state?.reachability === "unavailable";
  const canControl = state?.ownership === "console";
  const busy = loading || action !== null;

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
      </div>
    </SectionPanel>
  );
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
