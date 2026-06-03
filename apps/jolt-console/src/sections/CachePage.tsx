import { MetricCard, MetricGrid, SectionPanel } from "../components/primitives";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";
import { formatBytes, value } from "../utils/format";

export function CachePage({ snapshot }: { snapshot: DaemonSnapshot }) {
  const cacheStats = snapshot.cacheStats ?? {};

  return (
    <SectionPanel eyebrow="Cache" summary="storage pressure" hero>
      <MetricGrid>
        <MetricCard label="Cached bytes" value={formatBytes(cacheStats.total_cached)} />
        <MetricCard label="Published bytes" value={formatBytes(cacheStats.total_published)} />
        <MetricCard label="Pinned items" value={value(cacheStats.pinned_items)} />
        <MetricCard label="Available" value={formatBytes(cacheStats.available)} />
      </MetricGrid>
    </SectionPanel>
  );
}
