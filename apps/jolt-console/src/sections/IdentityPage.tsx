import { useState } from "react";
import { DetailGrid, DetailRow, SectionPanel } from "../components/primitives";
import {
  createLocalIdentity,
  selectLocalIdentity,
  type DaemonClient
} from "../daemon/client";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";

export function IdentityPage({
  client,
  snapshot
}: {
  client: DaemonClient;
  snapshot: DaemonSnapshot;
}) {
  const status = snapshot.status ?? {};
  const localIdentities = snapshot.localIdentities;
  const identities = localIdentities?.identities ?? [];
  const activeIdentity = localIdentities?.active_identity ?? status.identity_address ?? null;
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function createIdentity() {
    setBusy(true);
    setError(null);
    try {
      await createLocalIdentity(client, `Identity ${identities.length + 1}`);
      await snapshot.refresh();
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : String(nextError));
    } finally {
      setBusy(false);
    }
  }

  async function selectIdentity(identity: string) {
    setBusy(true);
    setError(null);
    try {
      await selectLocalIdentity(client, identity);
      await snapshot.refresh();
    } catch (nextError) {
      setError(nextError instanceof Error ? nextError.message : String(nextError));
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <SectionPanel eyebrow="Identity" summary="current daemon identity" hero>
        <DetailGrid>
          <DetailRow label="Active local identity" value={activeIdentity ?? "--"} />
          <DetailRow label="Daemon signing identity" value={status.identity_address ?? "--"} />
          <DetailRow label="Peer ID" value={status.peer_id ?? "--"} />
        </DetailGrid>
      </SectionPanel>

      <SectionPanel eyebrow="Local identities" summary={`${identities.length} available`}>
        {error ? <div className="identity-error">{error}</div> : null}
        <div className="identity-toolbar">
          <button type="button" onClick={createIdentity} disabled={busy}>
            Create identity
          </button>
        </div>
        <div className="identity-list">
          {identities.map((identity) => (
            <div className={`identity-row ${identity.active ? "active" : ""}`} key={identity.address}>
              <div>
                <strong className="mono">{identity.address}</strong>
                <span>{identity.label ?? "Local identity"}</span>
              </div>
              <button
                type="button"
                onClick={() => selectIdentity(identity.address)}
                disabled={busy || identity.active}
              >
                {identity.active ? "Active" : "Select"}
              </button>
            </div>
          ))}
        </div>
      </SectionPanel>
    </>
  );
}
