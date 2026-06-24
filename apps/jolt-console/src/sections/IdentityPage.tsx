import { useState, type FormEvent } from "react";
import { DetailGrid, DetailRow, SectionPanel } from "../components/primitives";
import {
  createLocalIdentity,
  deleteLocalIdentity,
  exportIdentity,
  importIdentity,
  selectLocalIdentity,
  tauriIdentityRecoveryFileClient,
  type DaemonClient,
  type IdentityRecoveryFileClient,
} from "../daemon/client";
import {
  tauriDaemonLifecycleClient,
  type DaemonLifecycleClient,
} from "../daemon/lifecycle";
import type { IdentityExportBundle } from "../daemon/types";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";

export function IdentityPage({
  client,
  snapshot,
  recoveryFileClient = tauriIdentityRecoveryFileClient,
  lifecycleClient = tauriDaemonLifecycleClient,
}: {
  client: DaemonClient;
  snapshot: DaemonSnapshot;
  recoveryFileClient?: IdentityRecoveryFileClient;
  lifecycleClient?: DaemonLifecycleClient;
}) {
  const status = snapshot.status ?? {};
  const localIdentities = snapshot.localIdentities;
  const identities = localIdentities?.identities ?? [];
  const activeIdentity =
    localIdentities?.active_identity ?? status.identity_address ?? null;
  const activeRecord = identities.find(
    (identity) => identity.address === activeIdentity,
  );
  const activeName = activeRecord?.label?.trim() || "Unnamed identity";
  const daemonIdentity = status.identity_address ?? null;
  const activeValue = activeIdentity
    ? `${activeName} (${activeIdentity})`
    : "--";
  const [identityName, setIdentityName] = useState("");
  const [exportPassphrase, setExportPassphrase] = useState("");
  const [exportLabel, setExportLabel] = useState("");
  const [importPassphrase, setImportPassphrase] = useState("");
  const [pendingImportBundle, setPendingImportBundle] =
    useState<IdentityExportBundle | null>(null);
  const [importNeedsPassphrase, setImportNeedsPassphrase] = useState(false);
  const [recoveryStatus, setRecoveryStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function createIdentity(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const label = identityName.trim();
    if (!label) {
      setError("Identity name is required.");
      return;
    }

    setBusy(true);
    setError(null);
    try {
      await createLocalIdentity(client, label);
      setIdentityName("");
      await snapshot.refresh();
    } catch (nextError) {
      setError(
        nextError instanceof Error ? nextError.message : String(nextError),
      );
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
      setError(
        nextError instanceof Error ? nextError.message : String(nextError),
      );
    } finally {
      setBusy(false);
    }
  }

  async function deleteIdentity(identity: string) {
    setBusy(true);
    setError(null);
    try {
      await deleteLocalIdentity(client, identity);
      await snapshot.refresh();
    } catch (nextError) {
      setError(
        nextError instanceof Error ? nextError.message : String(nextError),
      );
    } finally {
      setBusy(false);
    }
  }

  async function exportDaemonIdentity(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setRecoveryStatus(null);
    try {
      const response = await exportIdentity(
        client,
        exportPassphrase,
        exportLabel.trim(),
      );
      const path = await recoveryFileClient.save(
        response.identity,
        response.bundle,
      );
      if (path) {
        setRecoveryStatus(`Exported ${response.identity} to ${path}.`);
      } else {
        setRecoveryStatus("Export cancelled.");
      }
    } catch (nextError) {
      setError(
        nextError instanceof Error ? nextError.message : String(nextError),
      );
    } finally {
      setBusy(false);
    }
  }

  async function importDaemonIdentity(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setBusy(true);
    setError(null);
    setRecoveryStatus(null);
    let bundle = pendingImportBundle;
    try {
      if (!bundle) {
        bundle = await recoveryFileClient.open();
      }
      if (!bundle) {
        setRecoveryStatus("Import cancelled.");
        return;
      }
      const response = await importIdentity(
        client,
        bundle,
        importPassphrase,
        false,
        true,
      );
      if (response.restart_required) {
        await lifecycleClient.restart();
        setRecoveryStatus(
          `Imported ${response.identity} and restarted the daemon.`,
        );
      } else {
        setRecoveryStatus(`Imported ${response.identity} as a local identity.`);
      }
      setImportNeedsPassphrase(false);
      setPendingImportBundle(null);
      setImportPassphrase("");
      await snapshot.refresh();
    } catch (nextError) {
      if (bundle && isIdentityExportPassphraseError(nextError)) {
        setPendingImportBundle(bundle);
        setImportNeedsPassphrase(true);
        setRecoveryStatus("This identity export needs a passphrase.");
        if (importNeedsPassphrase) {
          setError("That passphrase did not unlock the identity export.");
        }
        return;
      }
      setError(
        nextError instanceof Error ? nextError.message : String(nextError),
      );
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <SectionPanel eyebrow="Identity" summary="current daemon identity" hero>
        <DetailGrid>
          <DetailRow label="Active local identity" value={activeValue} />
          <DetailRow
            label="Daemon signing identity"
            value={status.identity_address ?? "--"}
          />
          <DetailRow label="Peer ID" value={status.peer_id ?? "--"} />
        </DetailGrid>
      </SectionPanel>

      <SectionPanel
        eyebrow="Local identities"
        summary={`${identities.length} available`}
      >
        {error ? <div className="identity-error">{error}</div> : null}
        <form className="identity-toolbar" onSubmit={createIdentity}>
          <label htmlFor="identity-name">Identity name</label>
          <input
            id="identity-name"
            type="text"
            value={identityName}
            onChange={(event) => setIdentityName(event.target.value)}
            disabled={busy}
            placeholder="Work"
          />
          <button type="submit" disabled={busy}>
            Create identity
          </button>
        </form>
        <div className="identity-table-wrap">
          <table className="identity-table">
            <thead>
              <tr>
                <th scope="col">Name</th>
                <th scope="col">Identity</th>
                <th scope="col">Type</th>
                <th scope="col">Status</th>
                <th scope="col">Actions</th>
              </tr>
            </thead>
            <tbody>
              {identities.map((identity) => {
                const name = identity.label?.trim() || "Unnamed identity";
                const isDaemonIdentity = identity.address === daemonIdentity;
                return (
                  <tr
                    className={identity.active ? "active" : ""}
                    key={identity.address}
                  >
                    <td>{name}</td>
                    <td className="mono">{identity.address}</td>
                    <td>{isDaemonIdentity ? "Daemon key" : "Local key"}</td>
                    <td>
                      <span
                        className={`identity-status ${identity.active ? "active" : ""}`}
                      >
                        {identity.active ? "Active" : "Available"}
                      </span>
                    </td>
                    <td>
                      <div className="identity-actions">
                        <button
                          type="button"
                          onClick={() => selectIdentity(identity.address)}
                          disabled={busy || identity.active}
                          aria-label={`Assume ${name}`}
                        >
                          {identity.active ? "Active" : "Assume"}
                        </button>
                        <button
                          type="button"
                          onClick={() => deleteIdentity(identity.address)}
                          disabled={busy || isDaemonIdentity}
                          aria-label={`Delete ${name}`}
                        >
                          Delete
                        </button>
                      </div>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </SectionPanel>

      <SectionPanel
        eyebrow="Recovery"
        summary="export or import daemon identity"
      >
        {recoveryStatus ? (
          <div className="identity-status-message">{recoveryStatus}</div>
        ) : null}
        <div className="identity-recovery-grid">
          <form
            className="identity-recovery-form"
            onSubmit={exportDaemonIdentity}
          >
            <div>
              <h2>Export daemon identity</h2>
              <p>
                Anyone with the export file can become this identity unless you
                add a passphrase.
              </p>
            </div>
            <label>
              Label
              <input
                type="text"
                value={exportLabel}
                onChange={(event) => setExportLabel(event.target.value)}
                disabled={busy}
                placeholder="Laptop"
              />
            </label>
            <label>
              Passphrase (optional)
              <input
                type="password"
                value={exportPassphrase}
                onChange={(event) => setExportPassphrase(event.target.value)}
                disabled={busy}
              />
            </label>
            <button type="submit" disabled={busy}>
              Export identity
            </button>
          </form>

          <form
            className="identity-recovery-form"
            onSubmit={importDaemonIdentity}
          >
            <div>
              <h2>Import daemon identity</h2>
              <p>
                Import validates the bundle and adds it as a local identity
                without replacing the daemon identity.
              </p>
            </div>
            {importNeedsPassphrase ? (
              <label>
                Passphrase
                <input
                  type="password"
                  value={importPassphrase}
                  onChange={(event) => setImportPassphrase(event.target.value)}
                  disabled={busy}
                  autoFocus
                />
              </label>
            ) : null}
            <button type="submit" disabled={busy}>
              {importNeedsPassphrase ? "Unlock and import" : "Import identity"}
            </button>
          </form>
        </div>
      </SectionPanel>
    </>
  );
}

function isIdentityExportPassphraseError(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  return (
    message.includes("identity_recovery_invalid") &&
    message.includes("identity export passphrase is incorrect")
  );
}
