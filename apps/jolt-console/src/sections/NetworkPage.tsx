import { DetailGrid, DetailRow, SectionPanel } from "../components/primitives";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";
import { value } from "../utils/format";

export function NetworkPage({ snapshot }: { snapshot: DaemonSnapshot }) {
  const status = snapshot.status ?? {};

  return (
    <SectionPanel eyebrow="Network" summary="peers and reachability" hero>
      <DetailGrid>
        <DetailRow label="Direct peers" value={value(status.direct_peers)} />
        <DetailRow label="Relayed peers" value={value(status.relayed_peers)} />
        <DetailRow label="Active relays" value={value(status.active_relays)} />
        <DetailRow label="Bootstrap peers" value={value(status.connected_bootstrap_peers)} />
      </DetailGrid>
    </SectionPanel>
  );
}
