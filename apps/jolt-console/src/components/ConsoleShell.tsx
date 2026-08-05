import { NavLink, useLocation } from "react-router-dom";
import { consoleRoutes } from "../app/navigation";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";
import type { ConsoleUpdateCheck } from "../update/client";

type ConsoleShellProps = {
  children: React.ReactNode;
  snapshot: DaemonSnapshot;
  consoleVersion: string;
  updateCheck?: ConsoleUpdateCheck | null;
};

export function ConsoleShell({
  children,
  snapshot,
  consoleVersion,
  updateCheck = null
}: ConsoleShellProps) {
  const location = useLocation();
  const currentRoute =
    consoleRoutes.find((route) => route.path === location.pathname) ?? consoleRoutes[0];
  const daemonVersion = snapshot.status?.daemon_version ?? "unknown";

  return (
    <div className="console-shell">
      <aside className="sidebar" aria-label="Jolt Console sections">
        <div className="brand-lockup">
          <svg viewBox="0 0 64 64" className="brand-mark" aria-hidden="true">
            <rect width="64" height="64" rx="12" fill="#0b0d0c" />
            <path d="M17 43 25 17h8l-8 26zm15 0 8-26h8l-8 26z" fill="#d9ff43" />
          </svg>
          <div>
            <strong>Jolt Console</strong>
            <span>local daemon control</span>
          </div>
        </div>

        <nav className="section-nav">
          {consoleRoutes.map((route) => (
            <NavLink key={route.id} to={route.path} end={route.path === "/"}>
              {route.label}
            </NavLink>
          ))}
        </nav>

        <div className="daemon-card">
          <span className="eyebrow">Daemon</span>
          <strong>{snapshot.connected ? "Connected" : "Disconnected"}</strong>
          <span className="mono">{snapshot.daemonUrl}</span>
          <div className="version-list" aria-label="Runtime versions">
            <span>Console v{consoleVersion}</span>
            <span>Daemon v{daemonVersion}</span>
          </div>
        </div>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">First-party trust surface</p>
            <h1>{currentRoute.label}</h1>
          </div>
          <div className="topbar-actions">
            {updateCheck?.available ? (
              <NavLink className="status-pill pending" to="/settings">
                Update {updateCheck.version}
              </NavLink>
            ) : null}
            <span className={`status-pill ${snapshot.connected ? "ok" : "pending"}`}>
              {snapshot.connected ? "connected" : "offline"}
            </span>
            <button type="button" onClick={() => void snapshot.refresh()}>
              Refresh
            </button>
          </div>
        </header>

        {children}
      </main>
    </div>
  );
}
