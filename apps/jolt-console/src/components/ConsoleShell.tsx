import { NavLink, useLocation } from "react-router-dom";
import { consoleRoutes } from "../app/navigation";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";

type ConsoleShellProps = {
  children: React.ReactNode;
  snapshot: DaemonSnapshot;
};

export function ConsoleShell({ children, snapshot }: ConsoleShellProps) {
  const location = useLocation();
  const currentRoute =
    consoleRoutes.find((route) => route.path === location.pathname) ?? consoleRoutes[0];

  return (
    <div className="console-shell">
      <aside className="sidebar" aria-label="Jolt Console sections">
        <div className="brand-lockup">
          <div className="brand-mark">J</div>
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
        </div>
      </aside>

      <main className="workspace">
        <header className="topbar">
          <div>
            <p className="eyebrow">First-party trust surface</p>
            <h1>{currentRoute.label}</h1>
          </div>
          <div className="topbar-actions">
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
