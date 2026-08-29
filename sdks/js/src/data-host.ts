import { createDataAppClient } from "./client.js";
import type { JoltClient } from "./client.js";
import {
  AppIncompatibleError,
  AppSessionRejectedError,
} from "./data-errors.js";
import type {
  AppAccessPlan,
  DataSdkClient,
  Identity,
} from "./data.js";

const DATA_RECORDS_LEVEL = 5;
const DATA_SUBSCRIPTIONS_LEVEL = 1;
const DATA_CHANGE_STREAMS_LEVEL = 1;
const fallbackStorage = new Map<string, string>();

/** @internal Client behavior used while the Data SDK establishes an App session. */
export type DataAppHostClient = DataSdkClient & Pick<
  JoltClient,
  | "checkCompatibility"
  | "getStatus"
  | "getCurrentSession"
  | "requestSession"
  | "getSessionRequestStatus"
>;

/** @internal Minimal persistent session-token storage. */
export type DataAppSessionStorage = {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
};

type DataAppHostDefinition = {
  readonly id: string;
  readonly name: string;
  readonly accessPlan: AppAccessPlan;
};

type DataAppHostDependencies = {
  readonly createClient: (
    getSessionToken: () => string,
  ) => DataAppHostClient | Promise<DataAppHostClient>;
  readonly storage: DataAppSessionStorage;
  readonly appOrigin: string;
  readonly sleep: (milliseconds: number) => Promise<void>;
};

function unique(values: readonly string[]): string[] {
  return [...new Set(values)];
}

/** @internal Derives low-level session authority from high-level Resource access. */
export function capabilitiesForDataApp(accessPlan: AppAccessPlan): string[] {
  const capabilities = ["resolve:public", "fetch:public"];
  for (const grant of accessPlan.grants) {
    if (
      grant.access.create === true
      || grant.access.update === true
      || grant.access.restore === true
    ) {
      capabilities.push(`publish:${grant.path}`);
    }
    if (grant.access.delete === true) {
      capabilities.push(`delete:${grant.path}`);
    }
  }
  for (const subscription of accessPlan.subscriptions) {
    capabilities.push(`subscribe:any:${subscription.path}`);
  }
  return unique(capabilities);
}

function storedSessionStillApplies(
  session: Awaited<ReturnType<DataAppHostClient["getCurrentSession"]>>,
  app: DataAppHostDefinition,
  identity: Identity,
  capabilities: readonly string[],
): boolean {
  const granted = new Set(session.granted_capabilities);
  return session.status === "active"
    && session.app_id === app.id
    && session.identity === identity
    && capabilities.every(capability => granted.has(capability));
}

function sessionStorageKey(appId: string): string {
  return `jolt.data.session:${appId}`;
}

function browserStorage(): DataAppSessionStorage {
  try {
    if (typeof localStorage !== "undefined") {
      const probe = "jolt.data.storage.probe";
      localStorage.setItem(probe, probe);
      localStorage.removeItem(probe);
      return localStorage;
    }
  } catch {
    // Sandboxed webviews may expose localStorage but refuse access to it.
  }
  return {
    getItem: key => fallbackStorage.get(key) ?? null,
    setItem: (key, value) => { fallbackStorage.set(key, value); },
    removeItem: key => { fallbackStorage.delete(key); },
  };
}

function browserOrigin(): string {
  return typeof location !== "undefined" && location.origin !== "null"
    ? location.origin
    : "jolt-sdk://local";
}

function isTauriHost(): boolean {
  if (typeof window === "undefined") return false;
  const internals = (window as typeof window & {
    __TAURI_INTERNALS__?: { invoke?: unknown };
  }).__TAURI_INTERNALS__;
  return typeof internals?.invoke === "function";
}

async function createDefaultClient(
  getSessionToken: () => string,
): Promise<DataAppHostClient> {
  if (isTauriHost()) {
    const { TauriTransport } = await import("./transport-tauri.js");
    return createDataAppClient({
      transport: new TauriTransport({ plugin: true }),
      getSessionToken,
    });
  }
  const { HttpTransport } = await import("./transport-http.js");
  return createDataAppClient({
    transport: new HttpTransport({}),
    getSessionToken,
  });
}

function defaultDependencies(): DataAppHostDependencies {
  return {
    createClient: createDefaultClient,
    storage: browserStorage(),
    appOrigin: browserOrigin(),
    sleep: milliseconds => new Promise(resolve => setTimeout(resolve, milliseconds)),
  };
}

/** @internal Establishes the already-authorized seam consumed by App.connect(). */
export async function connectDataApp(
  app: DataAppHostDefinition,
  dependencies: DataAppHostDependencies = defaultDependencies(),
): Promise<{ readonly identity: Identity; readonly client: DataSdkClient }> {
  const key = sessionStorageKey(app.id);
  let token = dependencies.storage.getItem(key) ?? "";
  const client = await dependencies.createClient(() => token);
  const compatibility = await client.checkCompatibility({
    appApi: 1,
    requiredFeatures: {
      "data.records": DATA_RECORDS_LEVEL,
      ...(app.accessPlan.subscriptions.length > 0
        ? {
            "data.change-streams": DATA_CHANGE_STREAMS_LEVEL,
            "data.subscriptions": DATA_SUBSCRIPTIONS_LEVEL,
          }
        : {}),
    },
  });
  if (compatibility.status !== "compatible") {
    throw new AppIncompatibleError(compatibility);
  }

  const identity = (await client.getStatus()).identity_address;
  const capabilities = capabilitiesForDataApp(app.accessPlan);
  if (token) {
    try {
      const session = await client.getCurrentSession();
      if (storedSessionStillApplies(session, app, identity, capabilities)) {
        return { identity, client };
      }
    } catch {
      // A missing, expired, or revoked token simply needs fresh approval.
    }
    token = "";
    dependencies.storage.removeItem(key);
  }

  const request = await client.requestSession({
    appId: app.id,
    appName: app.name,
    appOrigin: dependencies.appOrigin,
    identity,
    capabilities,
  });
  for (;;) {
    const status = await client.getSessionRequestStatus(request.request_id);
    if (
      status.status === "rejected"
      || status.status === "revoked"
      || status.status === "expired"
    ) {
      throw new AppSessionRejectedError(status.status);
    }
    if (status.status === "active" && status.session_token) {
      token = status.session_token;
      dependencies.storage.setItem(key, token);
      return { identity, client };
    }
    await dependencies.sleep(1_000);
  }
}
