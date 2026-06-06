import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  loadAppPermissions,
  tauriDaemonClient,
  type DaemonClient
} from "../daemon/client";
import type { AppPermissionsPayload, AppSessionGrant } from "../daemon/types";
import { SectionPanel } from "../components/primitives";

type AppsPageProps = {
  client?: DaemonClient;
  refreshIntervalMs?: number;
};

type CapabilityInfo = {
  label: string;
  kind: "read" | "write" | "pin" | "inventory" | "blocked";
  grantable: boolean;
  broadPath: boolean;
};

const EMPTY_PERMISSIONS: AppPermissionsPayload = {
  requests: [],
  sessions: []
};

export function AppsPage({ client = tauriDaemonClient, refreshIntervalMs = 5000 }: AppsPageProps) {
  const [permissions, setPermissions] = useState<AppPermissionsPayload>(EMPTY_PERMISSIONS);
  const [loading, setLoading] = useState(true);
  const [action, setAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [expandedRows, setExpandedRows] = useState<Set<string>>(new Set());
  const actionRef = useRef<string | null>(null);
  const failureCount = useRef(0);
  actionRef.current = action;

  const refresh = useCallback(async ({ showLoading = true }: { showLoading?: boolean } = {}) => {
    if (showLoading) {
      setLoading(true);
    }
    setError(null);
    try {
      setPermissions(await loadAppPermissions(client));
      failureCount.current = 0;
      return true;
    } catch (err) {
      failureCount.current += 1;
      setError(err instanceof Error ? err.message : String(err));
      return false;
    } finally {
      if (showLoading) {
        setLoading(false);
      }
    }
  }, [client]);

  useEffect(() => {
    let cancelled = false;
    let timeout: number | undefined;
    const nextDelay = () =>
      failureCount.current === 0
        ? refreshIntervalMs
        : Math.min(refreshIntervalMs * 2 ** failureCount.current, 30000);
    const schedule = () => {
      if (refreshIntervalMs <= 0) return;
      timeout = window.setTimeout(async () => {
        if (actionRef.current === null) {
          await refresh({ showLoading: false });
        }
        if (!cancelled) {
          schedule();
        }
      }, nextDelay());
    };

    void (async () => {
      await refresh();
      if (!cancelled) {
        schedule();
      }
    })();

    return () => {
      cancelled = true;
      if (timeout !== undefined) {
        window.clearTimeout(timeout);
      }
    };
  }, [refresh, refreshIntervalMs]);

  async function runAction(label: string, operation: () => Promise<unknown>) {
    setAction(label);
    setError(null);
    try {
      await operation();
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setAction(null);
    }
  }

  const pending = useMemo(
    () =>
      permissions.requests
        .filter((request) => request.status === "pending")
        .sort(compareGrantRecency),
    [permissions.requests]
  );
  const sessions = useMemo(
    () => [...permissions.sessions].sort(compareGrantRecency),
    [permissions.sessions]
  );

  function toggleRow(rowId: string) {
    setExpandedRows((current) => {
      const next = new Set(current);
      if (next.has(rowId)) {
        next.delete(rowId);
      } else {
        next.add(rowId);
      }
      return next;
    });
  }

  return (
    <SectionPanel eyebrow="Apps" summary="approve, reject, and revoke app authority" hero>
      <div className="permission-api-note">
        <span className="mono">/admin/v1/app-requests</span>
        <span className="mono">/admin/v1/app-sessions</span>
        <button type="button" onClick={() => void refresh()} disabled={loading || action !== null}>
          Refresh
        </button>
      </div>
      {error ? <div className="permission-error">Permission API error: {error}</div> : null}
      <div className="permission-layout">
        <PermissionColumn title="Pending requests">
          {loading && pending.length === 0 ? <EmptyState label="Loading app requests..." /> : null}
          {!loading && pending.length === 0 ? <EmptyState label="No pending app requests." /> : null}
          {pending.map((request) => (
            <PermissionRequestCard
              key={request.request_id}
              request={request}
              busy={action !== null}
              expanded={expandedRows.has(rowKey("request", request))}
              onToggle={() => toggleRow(rowKey("request", request))}
              onApprove={() =>
                runAction(`approve-${request.request_id}`, () =>
                  approveRequest(client, request)
                )
              }
              onReject={() =>
                runAction(`reject-${request.request_id}`, () =>
                  client.post(`/admin/v1/app-requests/${request.request_id}/reject`)
                )
              }
            />
          ))}
        </PermissionColumn>

        <PermissionColumn title="App sessions">
          {loading && sessions.length === 0 ? <EmptyState label="Loading app sessions..." /> : null}
          {!loading && sessions.length === 0 ? <EmptyState label="No app sessions yet." /> : null}
          {sessions.map((session) => (
            <SessionCard
              key={session.session_id ?? session.request_id}
              session={session}
              busy={action !== null}
              expanded={expandedRows.has(rowKey("session", session))}
              onToggle={() => toggleRow(rowKey("session", session))}
              onRevoke={() => {
                if (!session.session_id) return Promise.resolve();
                return runAction(`revoke-${session.session_id}`, () =>
                  client.post(`/admin/v1/app-sessions/${session.session_id}/revoke`)
                );
              }}
            />
          ))}
        </PermissionColumn>
      </div>
    </SectionPanel>
  );
}

function PermissionColumn({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="permission-column">
      <h2>{title}</h2>
      <div className="permission-stack">{children}</div>
    </div>
  );
}

function PermissionRequestCard({
  request,
  busy,
  expanded,
  onToggle,
  onApprove,
  onReject
}: {
  request: AppSessionGrant;
  busy: boolean;
  expanded: boolean;
  onToggle: () => void;
  onApprove: () => Promise<unknown>;
  onReject: () => Promise<unknown>;
}) {
  const capabilities = useMemo(
    () => request.requested_capabilities.map(capabilityInfo),
    [request.requested_capabilities]
  );
  const blocked = capabilities.some((capability) => !capability.grantable);
  const identity = request.requested_identity ?? "a selected identity";

  return (
    <article className={`permission-row pending ${expanded ? "expanded" : ""}`}>
      <div className="permission-row-header">
        <PermissionSummaryButton
          grant={request}
          label="request details"
          expanded={expanded}
          onToggle={onToggle}
          detail={`${request.app_name} wants ${capabilities.length} capabilities for ${identity}`}
        />
        <div className="permission-quick-actions">
        <button
          type="button"
          aria-label={`Approve ${request.app_name}`}
          onClick={onApprove}
          disabled={busy || blocked}
        >
          Approve
        </button>
        <button
          type="button"
          className="danger-button"
          aria-label={`Reject ${request.app_name}`}
          onClick={onReject}
          disabled={busy}
        >
          Reject
        </button>
        </div>
      </div>
      {expanded ? (
        <div className="permission-row-body">
          <p className="permission-intent">
            {request.app_name} wants to use <span className="mono">{identity}</span> for this authority.
          </p>
          <CapabilityList capabilities={capabilities} />
          {blocked ? (
            <div className="permission-warning">admin-only request: cannot be approved</div>
          ) : null}
          <div className="permission-meta">
            <span>Created {formatTimestamp(request.created_at)}</span>
            {request.expires_at ? <span>Expires {formatTimestamp(request.expires_at)}</span> : null}
          </div>
        </div>
      ) : null}
    </article>
  );
}

function SessionCard({
  session,
  busy,
  expanded,
  onToggle,
  onRevoke
}: {
  session: AppSessionGrant;
  busy: boolean;
  expanded: boolean;
  onToggle: () => void;
  onRevoke: () => Promise<unknown>;
}) {
  const capabilities = session.granted_capabilities.map(capabilityInfo);
  const revocable = session.status === "active" && Boolean(session.session_id);

  return (
    <article className={`permission-row ${session.status} ${expanded ? "expanded" : ""}`}>
      <div className="permission-row-header">
        <PermissionSummaryButton
          grant={session}
          label="session details"
          expanded={expanded}
          onToggle={onToggle}
          detail={`${capabilities.length} capabilities granted to ${session.identity ?? session.requested_identity ?? "--"}`}
        />
        <div className="permission-quick-actions">
        <button
          type="button"
          className="danger-button"
          aria-label={`Revoke ${session.app_name}`}
          onClick={onRevoke}
          disabled={busy || !revocable}
        >
          Revoke
        </button>
        </div>
      </div>
      {expanded ? (
        <div className="permission-row-body">
          <p className="permission-intent">
            Granted identity <span className="mono">{session.identity ?? session.requested_identity ?? "--"}</span>
          </p>
          <CapabilityList capabilities={capabilities} />
          <div className="permission-meta">
            <span>Created {formatTimestamp(session.created_at)}</span>
            {session.last_used_at ? <span>Last used {formatTimestamp(session.last_used_at)}</span> : null}
            {session.expires_at ? <span>Expires {formatTimestamp(session.expires_at)}</span> : null}
          </div>
        </div>
      ) : null}
    </article>
  );
}

function PermissionSummaryButton({
  grant,
  label,
  expanded,
  onToggle,
  detail
}: {
  grant: AppSessionGrant;
  label: string;
  expanded: boolean;
  onToggle: () => void;
  detail: string;
}) {
  return (
    <button
      type="button"
      className="permission-summary-button"
      aria-expanded={expanded}
      onClick={onToggle}
    >
      <span className="permission-disclosure" aria-hidden="true">
        {expanded ? "-" : "+"}
      </span>
      <div className="permission-summary-copy">
        <span className="permission-summary-title">
          <strong>{grant.app_name}</strong>
          <span className={`grant-status ${grant.status}`}>{grant.status}</span>
        </span>
        <span className="mono">{grant.app_origin ?? grant.app_id}</span>
        <span className="permission-summary-detail">{detail}</span>
      </div>
      <span className="visually-hidden">{label}</span>
    </button>
  );
}

function CapabilityList({ capabilities }: { capabilities: CapabilityInfo[] }) {
  return (
    <ul className="capability-list">
      {capabilities.map((capability, index) => (
        <li key={`${capability.label}-${index}`} className={`capability-row ${capability.kind}`}>
          <span>{capability.label}</span>
          {capability.broadPath ? <strong>Broad path scope</strong> : null}
        </li>
      ))}
    </ul>
  );
}

function EmptyState({ label }: { label: string }) {
  return <div className="permission-empty">{label}</div>;
}

function rowKey(kind: "request" | "session", grant: AppSessionGrant) {
  return `${kind}:${grant.session_id ?? grant.request_id}`;
}

function compareGrantRecency(a: AppSessionGrant, b: AppSessionGrant) {
  return grantRecency(b) - grantRecency(a);
}

function grantRecency(grant: AppSessionGrant) {
  return (
    grant.last_used_at ??
    grant.revoked_at ??
    grant.rejected_at ??
    grant.approved_at ??
    grant.created_at
  );
}

async function approveRequest(client: DaemonClient, request: AppSessionGrant) {
  const capabilities = request.requested_capabilities.filter(isGrantableCapability);
  return client.post(`/admin/v1/app-requests/${request.request_id}/approve`, {
    identity: request.requested_identity ?? request.identity ?? null,
    capabilities,
    expires_at: null
  });
}

function capabilityInfo(capability: string): CapabilityInfo {
  if (capability === "resolve:public") {
    return {
      label: "read public Jolt addresses",
      kind: "read",
      grantable: true,
      broadPath: false
    };
  }
  if (capability === "fetch:public") {
    return {
      label: "fetch public content",
      kind: "read",
      grantable: true,
      broadPath: false
    };
  }
  if (capability === "ingress:read") {
    return {
      label: "read pending incoming app objects",
      kind: "read",
      grantable: true,
      broadPath: false
    };
  }
  if (capability === "ingress:send") {
    return {
      label: "send incoming app objects by identity",
      kind: "write",
      grantable: true,
      broadPath: false
    };
  }
  if (capability === "ingress:decide") {
    return {
      label: "accept or reject pending incoming app objects",
      kind: "write",
      grantable: true,
      broadPath: false
    };
  }
  if (capability.startsWith("publish:encrypted:")) {
    const scope = capability.slice("publish:encrypted:".length);
    return {
      label: `publish encrypted content under ${scope}`,
      kind: "write",
      grantable: isGrantablePathCapability("publish:encrypted:", capability),
      broadPath: isBroadPathScope(scope)
    };
  }
  if (capability.startsWith("publish:")) {
    const scope = capability.slice("publish:".length);
    return {
      label: `create or update signed paths under ${scope}`,
      kind: "write",
      grantable: isGrantablePathCapability("publish:", capability),
      broadPath: isBroadPathScope(scope)
    };
  }
  if (capability.startsWith("inventory:")) {
    const scope = capability.slice("inventory:".length);
    return {
      label: `list local published content under ${scope}`,
      kind: "inventory",
      grantable: isGrantablePathCapability("inventory:", capability),
      broadPath: isBroadPathScope(scope)
    };
  }
  if (capability.startsWith("pin:own:")) {
    const scope = capability.slice("pin:own:".length);
    return {
      label: `pin content it publishes under ${scope}`,
      kind: "pin",
      grantable: isGrantablePathCapability("pin:own:", capability),
      broadPath: isBroadPathScope(scope)
    };
  }
  if (capability.startsWith("encrypt:")) {
    const scope = capability.slice("encrypt:".length);
    return {
      label: `encrypt content under ${scope}`,
      kind: "write",
      grantable: isGrantablePathCapability("encrypt:", capability),
      broadPath: isBroadPathScope(scope)
    };
  }
  if (capability.startsWith("decrypt:")) {
    const scope = capability.slice("decrypt:".length);
    return {
      label: `decrypt content under ${scope}`,
      kind: "read",
      grantable: isGrantablePathCapability("decrypt:", capability),
      broadPath: isBroadPathScope(scope)
    };
  }

  return {
    label: capability,
    kind: "blocked",
    grantable: false,
    broadPath: false
  };
}

function isGrantableCapability(capability: string) {
  return (
    capability === "resolve:public" ||
    capability === "fetch:public" ||
    capability === "ingress:send" ||
    capability === "ingress:read" ||
    capability === "ingress:decide" ||
    isGrantablePathCapability("publish:encrypted:", capability) ||
    isGrantablePathCapability("publish:", capability) ||
    isGrantablePathCapability("inventory:", capability) ||
    isGrantablePathCapability("pin:own:", capability) ||
    isGrantablePathCapability("encrypt:", capability) ||
    isGrantablePathCapability("decrypt:", capability)
  );
}

function isGrantablePathCapability(prefix: string, capability: string) {
  const scope = capability.slice(prefix.length);
  return capability.startsWith(prefix) && scope.startsWith("/") && !scope.includes("..");
}

function isBroadPathScope(scope: string) {
  return scope === "/*" || scope.endsWith("/*");
}

function formatTimestamp(seconds: number) {
  const date = new Date(seconds * 1000);
  const year = date.getUTCFullYear();
  const month = String(date.getUTCMonth() + 1).padStart(2, "0");
  const day = String(date.getUTCDate()).padStart(2, "0");
  const hours = String(date.getUTCHours()).padStart(2, "0");
  const minutes = String(date.getUTCMinutes()).padStart(2, "0");
  return `${year}-${month}-${day} ${hours}:${minutes}`;
}
