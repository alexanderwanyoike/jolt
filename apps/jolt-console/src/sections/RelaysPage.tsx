import { Placeholder, SectionPanel } from "../components/primitives";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";

export function RelaysPage({ snapshot }: { snapshot: DaemonSnapshot }) {
  const relay = snapshot.status?.home_relay;

  return (
    <SectionPanel eyebrow="Relays" summary="home relay state" hero>
      {!relay ? (
        <Placeholder>Home relay is not configured.</Placeholder>
      ) : (
        <div className="placeholder">
          <dl className="summary-list">
            <div>
              <dt>Capability</dt>
              <dd>{relay.capability ?? "unknown"}</dd>
            </div>
            <div>
              <dt>Peer</dt>
              <dd className="mono">{relay.peer_id ?? "--"}</dd>
            </div>
            <div>
              <dt>Address</dt>
              <dd className="mono">{relay.multiaddr ?? "--"}</dd>
            </div>
            <div>
              <dt>API</dt>
              <dd className="mono">{relay.api_url ?? "not configured"}</dd>
            </div>
          </dl>
        </div>
      )}
    </SectionPanel>
  );
}
