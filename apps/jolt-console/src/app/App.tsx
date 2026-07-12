import { useEffect, useState } from "react";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { ConsoleShell } from "../components/ConsoleShell";
import { tauriDaemonClient, type DaemonClient } from "../daemon/client";
import {
  tauriDaemonLifecycleClient,
  type DaemonLifecycleClient
} from "../daemon/lifecycle";
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
import {
  tauriConsoleUpdateClient,
  type ConsoleUpdateCheck,
  type ConsoleUpdateClient
} from "../update/client";
import { CONSOLE_VERSION } from "../version";

type ConsoleAppProps = {
  client?: DaemonClient;
  lifecycleClient?: DaemonLifecycleClient;
  updateClient?: ConsoleUpdateClient;
  consoleVersion?: string;
  refreshIntervalMs?: number;
};

export function ConsoleApp({
  client = tauriDaemonClient,
  lifecycleClient = tauriDaemonLifecycleClient,
  updateClient = tauriConsoleUpdateClient,
  consoleVersion = CONSOLE_VERSION,
  refreshIntervalMs = 5000
}: ConsoleAppProps) {
  const snapshot = useDaemonSnapshot(client, refreshIntervalMs);
  const [updateCheck, setUpdateCheck] = useState<ConsoleUpdateCheck | null>(null);

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const lifecycle = await lifecycleClient.status();
        if (lifecycle.reachability !== "unavailable" || lifecycle.ownership !== "none") {
          return;
        }

        await lifecycleClient.start();
        if (!cancelled) {
          await refreshSnapshotUntilConnected(snapshot.refresh);
        }
      } catch {
        // Snapshot polling and Settings lifecycle controls surface daemon failures.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [lifecycleClient, snapshot.refresh]);

  useEffect(() => {
    let cancelled = false;

    void (async () => {
      try {
        const nextUpdateCheck = await updateClient.check();
        if (!cancelled) setUpdateCheck(nextUpdateCheck);
      } catch {
        // Settings exposes manual update checks and any updater errors.
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [updateClient]);

  return (
    <HashRouter>
      <ConsoleShell
        snapshot={snapshot}
        consoleVersion={consoleVersion}
        updateCheck={updateCheck}
      >
        <Routes>
          <Route index element={<OverviewPage snapshot={snapshot} />} />
          <Route path="/identity" element={<IdentityPage client={client} snapshot={snapshot} />} />
          <Route
            path="/apps"
            element={<AppsPage client={client} refreshIntervalMs={refreshIntervalMs} />}
          />
          <Route path="/network" element={<NetworkPage snapshot={snapshot} />} />
          <Route path="/relays" element={<RelaysPage snapshot={snapshot} />} />
          <Route path="/published" element={<PublishedPage snapshot={snapshot} />} />
          <Route path="/cache" element={<CachePage snapshot={snapshot} />} />
          <Route
            path="/settings"
            element={
              <SettingsPage
                lifecycleClient={lifecycleClient}
                daemonClient={client}
                updateClient={updateClient}
              />
            }
          />
          <Route path="/diagnostics" element={<DiagnosticsPage snapshot={snapshot} />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </ConsoleShell>
    </HashRouter>
  );
}

async function refreshSnapshotUntilConnected(refresh: () => Promise<boolean>) {
  for (let attempt = 0; attempt < 10; attempt += 1) {
    if (await refresh()) return;
    await delay(500);
  }
}

function delay(ms: number) {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}
