import { DetailGrid, DetailRow, SectionPanel } from "../components/primitives";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";

export function IdentityPage({ snapshot }: { snapshot: DaemonSnapshot }) {
  const status = snapshot.status ?? {};

  return (
    <SectionPanel eyebrow="Identity" summary="current daemon identity" hero>
      <DetailGrid>
        <DetailRow label="Jolt address" value={status.identity_address ?? "--"} />
        <DetailRow label="Peer ID" value={status.peer_id ?? "--"} />
        <DetailRow label="NAT" value={status.nat_type ?? "--"} />
      </DetailGrid>
    </SectionPanel>
  );
}
