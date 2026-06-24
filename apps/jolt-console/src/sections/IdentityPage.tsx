import { useState, type FormEvent } from "react";
import { DetailGrid, DetailRow, SectionPanel } from "../components/primitives";
import {
  createLocalIdentity,
  deleteLocalIdentity,
  exportIdentity,
  importIdentity,
  selectLocalIdentity,
  type DaemonClient,
} from "../daemon/client";
import type { DaemonSnapshot } from "../daemon/useDaemonSnapshot";

export function IdentityPage({
  client,
  snapshot,
}: {
  client: DaemonClient;
  snapshot: DaemonSnapshot;
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
  const [exportRiskAccepted, setExportRiskAccepted] = useState(false);
  const [exportBundleJson, setExportBundleJson] = useState("");
  const [importBundleJson, setImportBundleJson] = useState("");
  const [importPassphrase, setImportPassphrase] = useState("");
  const [importAllowOverwrite, setImportAllowOverwrite] = useState(false);
  const [importRiskAccepted, setImportRiskAccepted] = useState(false);
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
      setExportBundleJson(JSON.stringify(response.bundle, null, 2));
      setRecoveryStatus(
        `Exported ${response.identity} with ${response.encryption_key_count} encryption key.`,
      );
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
    try {
      const bundle = JSON.parse(importBundleJson);
      const response = await importIdentity(
        client,
        bundle,
        importPassphrase,
        importAllowOverwrite,
      );
      setRecoveryStatus(
        response.restart_required
          ? `Imported ${response.identity}. Restart the daemon before using this identity.`
          : `Imported ${response.identity}.`,
      );
      await snapshot.refresh();
    } catch (nextError) {
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
                Anyone with the export file and passphrase can become this
                identity.
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
              Passphrase
              <input
                type="password"
                value={exportPassphrase}
                onChange={(event) => setExportPassphrase(event.target.value)}
                disabled={busy}
              />
            </label>
            <label className="identity-confirm">
              <input
                type="checkbox"
                checked={exportRiskAccepted}
                onChange={(event) =>
                  setExportRiskAccepted(event.target.checked)
                }
                disabled={busy}
              />
              I understand this exports private identity keys.
            </label>
            <button
              type="submit"
              disabled={busy || !exportRiskAccepted || !exportPassphrase}
            >
              Export identity
            </button>
            <label>
              Export bundle
              <textarea
                className="mono"
                value={exportBundleJson}
                onChange={(event) => setExportBundleJson(event.target.value)}
                rows={10}
                readOnly
              />
            </label>
          </form>

          <form
            className="identity-recovery-form"
            onSubmit={importDaemonIdentity}
          >
            <div>
              <h2>Import daemon identity</h2>
              <p>
                Import validates the bundle and never overwrites a different
                identity unless allowed.
              </p>
            </div>
            <label>
              Bundle JSON
              <textarea
                className="mono"
                value={importBundleJson}
                onChange={(event) => setImportBundleJson(event.target.value)}
                disabled={busy}
                rows={10}
              />
            </label>
            <label>
              Passphrase
              <input
                type="password"
                value={importPassphrase}
                onChange={(event) => setImportPassphrase(event.target.value)}
                disabled={busy}
              />
            </label>
            <label className="identity-confirm">
              <input
                type="checkbox"
                checked={importAllowOverwrite}
                onChange={(event) =>
                  setImportAllowOverwrite(event.target.checked)
                }
                disabled={busy}
              />
              Allow replacing the existing daemon identity.
            </label>
            <label className="identity-confirm">
              <input
                type="checkbox"
                checked={importRiskAccepted}
                onChange={(event) =>
                  setImportRiskAccepted(event.target.checked)
                }
                disabled={busy}
              />
              I understand this imports private identity keys.
            </label>
            <button
              type="submit"
              disabled={
                busy ||
                !importRiskAccepted ||
                !importBundleJson ||
                !importPassphrase
              }
            >
              Import identity
            </button>
          </form>
        </div>
      </SectionPanel>
    </>
  );
}
