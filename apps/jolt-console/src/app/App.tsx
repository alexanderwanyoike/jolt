import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { ConsoleShell } from "../components/ConsoleShell";
import { tauriDaemonClient, type DaemonClient } from "../daemon/client";
import { useDaemonSnapshot } from "../daemon/useDaemonSnapshot";
import { AppsPage } from "../sections/AppsPage";
import { CachePage } from "../sections/CachePage";
import { DiagnosticsPage } from "../sections/DiagnosticsPage";
import { IdentityPage } from "../sections/IdentityPage";
import { NetworkPage } from "../sections/NetworkPage";
import { OverviewPage } from "../sections/OverviewPage";
import { PublishedPage } from "../sections/PublishedPage";
import { RelaysPage } from "../sections/RelaysPage";
import { SettingsPage } from "../sections/SettingsPage";

type ConsoleAppProps = {
  client?: DaemonClient;
  refreshIntervalMs?: number;
};

export function ConsoleApp({
  client = tauriDaemonClient,
  refreshIntervalMs = 5000
}: ConsoleAppProps) {
  const snapshot = useDaemonSnapshot(client, refreshIntervalMs);

  return (
    <HashRouter>
      <ConsoleShell snapshot={snapshot}>
        <Routes>
          <Route index element={<OverviewPage snapshot={snapshot} />} />
          <Route path="/identity" element={<IdentityPage snapshot={snapshot} />} />
          <Route path="/apps" element={<AppsPage client={client} />} />
          <Route path="/network" element={<NetworkPage snapshot={snapshot} />} />
          <Route path="/relays" element={<RelaysPage snapshot={snapshot} />} />
          <Route path="/published" element={<PublishedPage snapshot={snapshot} />} />
          <Route path="/cache" element={<CachePage snapshot={snapshot} />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/diagnostics" element={<DiagnosticsPage snapshot={snapshot} />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </ConsoleShell>
    </HashRouter>
  );
}
