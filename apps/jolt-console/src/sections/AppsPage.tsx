import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  loadAppPermissions,
  tauriDaemonClient,
  type DaemonClient
} from "../daemon/client";
import type { AppPermissionsPayload, AppSessionGrant } from "../daemon/types";
import { SectionPanel } from "../components/primitives";

type AppsPageProps = {
  client?: DaemonClient;
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

export function AppsPage({ client = tauriDaemonClient }: AppsPageProps) {
  const [permissions, setPermissions] = useState<AppPermissionsPayload>(EMPTY_PERMISSIONS);
  const [loading, setLoading] = useState(true);
  const [action, setAction] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setPermissions(await loadAppPermissions(client));
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [client]);

  useEffect(() => {
    refresh();
  }, [refresh]);

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

  const pending = permissions.requests.filter((request) => request.status === "pending");
  const sessions = permissions.sessions;

  return (
    <SectionPanel eyebrow="Apps" summary="approve, reject, and revoke app authority" hero>
      <div className="permission-api-note">
        <span className="mono">/admin/v1/app-requests</span>
        <span className="mono">/admin/v1/app-sessions</span>
        <button type="button" onClick={refresh} disabled={loading || action !== null}>
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
  onApprove,
  onReject
}: {
  request: AppSessionGrant;
  busy: boolean;
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
    <article className="permission-card pending">
      <PermissionHeader grant={request} status={request.status} />
      <p className="permission-intent">
        {request.app_name} wants to use <span className="mono">{identity}</span> for this authority.
      </p>
      <CapabilityList capabilities={capabilities} />
      {blocked ? (
        <div className="permission-warning">admin-only request: cannot be approved</div>
      ) : null}
      <div className="permission-actions">
        <button type="button" onClick={onApprove} disabled={busy || blocked}>
          Approve {request.app_name}
        </button>
        <button type="button" className="danger-button" onClick={onReject} disabled={busy}>
          Reject {request.app_name}
        </button>
      </div>
    </article>
  );
}

function SessionCard({
  session,
  busy,
  onRevoke
}: {
  session: AppSessionGrant;
  busy: boolean;
  onRevoke: () => Promise<unknown>;
}) {
  const capabilities = session.granted_capabilities.map(capabilityInfo);
  const revocable = session.status === "active" && Boolean(session.session_id);

  return (
    <article className={`permission-card ${session.status}`}>
      <PermissionHeader grant={session} status={session.status} />
      <p className="permission-intent">
        Granted identity <span className="mono">{session.identity ?? session.requested_identity ?? "--"}</span>
      </p>
      <CapabilityList capabilities={capabilities} />
      <div className="permission-meta">
        <span>Created {formatTimestamp(session.created_at)}</span>
        {session.last_used_at ? <span>Last used {formatTimestamp(session.last_used_at)}</span> : null}
        {session.expires_at ? <span>Expires {formatTimestamp(session.expires_at)}</span> : null}
      </div>
      <div className="permission-actions">
        <button
          type="button"
          className="danger-button"
          onClick={onRevoke}
          disabled={busy || !revocable}
        >
          Revoke {session.app_name}
        </button>
      </div>
    </article>
  );
}

function PermissionHeader({
  grant,
  status
}: {
  grant: AppSessionGrant;
  status: AppSessionGrant["status"];
}) {
  return (
    <header className="permission-card-header">
      <div>
        <strong>{grant.app_name}</strong>
        <span className="mono">{grant.app_origin ?? grant.app_id}</span>
      </div>
      <span className={`grant-status ${status}`}>{status}</span>
    </header>
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
    isGrantablePathCapability("publish:", capability) ||
    isGrantablePathCapability("inventory:", capability) ||
    isGrantablePathCapability("pin:own:", capability)
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
