import { Placeholder, SectionPanel } from "../components/primitives";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";
import { formatBytes } from "../utils/format";

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
      <div className="diagnostics-grid">
        <section className="list-panel">
          <h2>Connected peers</h2>
          {snapshot.peers.length ? (
            snapshot.peers.map((peer) => (
              <div className="content-row" key={peer.peer_id}>
                <div>
                  <strong className="mono">{peer.peer_id}</strong>
                  <span className="mono">{peer.remote_addr}</span>
                </div>
                <span>{peer.is_relayed ? "relayed" : peer.transport}</span>
              </div>
            ))
          ) : (
            <Placeholder>No connected peers.</Placeholder>
          )}
        </section>

        <section className="list-panel">
          <h2>Cache entries</h2>
          {snapshot.cacheEntries.length ? (
            snapshot.cacheEntries.map((entry) => (
              <div className="content-row" key={entry.content_id}>
                <div>
                  <strong className="mono">{entry.content_id}</strong>
                  <span>{entry.pinned ? "pinned" : "unpinned"}</span>
                </div>
                <span>
                  {formatBytes(entry.size)} - {entry.pinned ? "pinned" : "unpinned"}
                </span>
              </div>
            ))
          ) : (
            <Placeholder>No cached entries.</Placeholder>
          )}
        </section>
      </div>
      <pre className="diagnostics-output">{JSON.stringify(payload, null, 2)}</pre>
    </SectionPanel>
  );
}
