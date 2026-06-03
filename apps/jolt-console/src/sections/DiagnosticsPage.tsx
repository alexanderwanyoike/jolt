import { SectionPanel } from "../components/primitives";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";

export function DiagnosticsPage({ snapshot }: { snapshot: DaemonSnapshot }) {
  const payload = snapshot.lastError
    ? { daemon_url: snapshot.daemonUrl, error: snapshot.lastError }
    : {
        daemon_url: snapshot.daemonUrl,
        status: snapshot.status,
        cache: snapshot.cacheStats
      };

  return (
    <SectionPanel eyebrow="Diagnostics" summary="raw connection state" hero>
      <pre className="diagnostics-output">{JSON.stringify(payload, null, 2)}</pre>
    </SectionPanel>
  );
}
