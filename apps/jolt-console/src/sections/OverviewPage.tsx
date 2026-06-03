import { MetricCard, MetricGrid, SectionPanel } from "../components/primitives";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";
import { formatDuration, value } from "../utils/format";

export function OverviewPage({ snapshot }: { snapshot: DaemonSnapshot }) {
  const status = snapshot.status ?? {};
  const lastRefresh = snapshot.lastRefresh ? snapshot.lastRefresh.toLocaleTimeString() : "not refreshed";

  return (
    <SectionPanel eyebrow="Overview" summary={lastRefresh} hero>
      <MetricGrid>
        <MetricCard label="Uptime" value={formatDuration(status.uptime_secs)} />
        <MetricCard label="Peers" value={value(status.connected_peers)} />
        <MetricCard label="Published" value={value(status.published_count)} />
        <MetricCard label="Cached" value={value(status.cached_count)} />
        <MetricCard label="Known relays" value={value(status.known_relay_count)} />
        <MetricCard label="Bootstrap" value={status.bootstrap_state ?? "--"} />
      </MetricGrid>
    </SectionPanel>
  );
}
