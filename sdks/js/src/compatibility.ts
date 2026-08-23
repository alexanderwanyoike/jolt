/** App API feature discovery and application compatibility evaluation. */

import { JoltApiError } from "./errors.js";
import * as ops from "./operations.js";
import type { CallOptions, JoltTransport } from "./transport.js";
import type { AppApiFeatureManifestResponse } from "./wire.js";

/** App-owned runtime requirements, independent of daemon release versions. */
export type AppCompatibilityDeclaration = {
  appApi: number;
  requiredFeatures?: Readonly<Record<string, number>>;
  optionalFeatures?: Readonly<Record<string, number>>;
};

/** JSON representation embedded in signed application update manifests. */
export type AppCompatibilityDeclarationWire = {
  app_api: number;
  required_features: Readonly<Record<string, number>>;
  optional_features: Readonly<Record<string, number>>;
};

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isPositiveInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isInteger(value) && value > 0;
}

function decodeFeatureMap(
  value: unknown,
  field: "required_features" | "optional_features"
): Readonly<Record<string, number>> {
  if (!isRecord(value) || !Object.values(value).every(isPositiveInteger)) {
    throw new TypeError(
      `App Compatibility Declaration ${field} must contain positive integers`
    );
  }
  return { ...value } as Record<string, number>;
}

/** Decode signed update metadata into the transport-independent SDK shape. */
export function decodeAppCompatibilityDeclaration(
  value: unknown
): AppCompatibilityDeclaration {
  if (!isRecord(value) || !isPositiveInteger(value.app_api)) {
    throw new TypeError("App Compatibility Declaration app_api must be a positive integer");
  }
  return {
    appApi: value.app_api,
    requiredFeatures: decodeFeatureMap(value.required_features, "required_features"),
    optionalFeatures: decodeFeatureMap(value.optional_features, "optional_features"),
  };
}

/** Compatibility call controls; refresh after daemon reconnection. */
export type CompatibilityCheckOptions = CallOptions & {
  refresh?: boolean;
};

/** One comparison between an application requirement and daemon support. */
export type ContractLevelCheck = {
  requiredLevel: number;
  availableLevel: number | null;
  supported: boolean;
};

/** Generic App API behavior available through the connected daemon. */
export type AppApiFeatureManifest = {
  appApi: number;
  features: Readonly<Record<string, number>>;
  discovery: "advertised" | "legacy";
};

/** Complete compatibility result; applications own optional fallback choices. */
export type AppCompatibilityResult = {
  status: "compatible" | "incompatible";
  manifest: AppApiFeatureManifest;
  appApi: ContractLevelCheck;
  requiredFeatures: Readonly<Record<string, ContractLevelCheck>>;
  optionalFeatures: Readonly<Record<string, ContractLevelCheck>>;
};

/** App API compatibility checks that require no app session. */
export interface JoltCompatibilitySdk {
  checkCompatibility(
    declaration: AppCompatibilityDeclaration,
    options?: CompatibilityCheckOptions
  ): Promise<AppCompatibilityResult>;
}

function checkLevel(requiredLevel: number, availableLevel: number | undefined): ContractLevelCheck {
  return {
    requiredLevel,
    availableLevel: availableLevel ?? null,
    supported: availableLevel !== undefined && availableLevel >= requiredLevel,
  };
}

/** @internal Shared by the daemon-backed client and deterministic fake. */
export function evaluateCompatibility(
  declaration: AppCompatibilityDeclaration,
  response: AppApiFeatureManifestResponse,
  discovery: AppApiFeatureManifest["discovery"] = "advertised"
): AppCompatibilityResult {
  const appApi = checkLevel(declaration.appApi, response.app_api);
  const requiredFeatures = Object.fromEntries(
    Object.entries(declaration.requiredFeatures ?? {}).map(([feature, level]) => [
      feature,
      checkLevel(level, response.features[feature]),
    ])
  );
  const optionalFeatures = Object.fromEntries(
    Object.entries(declaration.optionalFeatures ?? {}).map(([feature, level]) => [
      feature,
      checkLevel(level, response.features[feature]),
    ])
  );
  const compatible =
    appApi.supported && Object.values(requiredFeatures).every(({ supported }) => supported);

  return {
    status: compatible ? "compatible" : "incompatible",
    manifest: {
      appApi: response.app_api,
      features: { ...response.features },
      discovery,
    },
    appApi,
    requiredFeatures,
    optionalFeatures,
  };
}

/** Build a connection-scoped compatibility checker over one transport. */
export function createCompatibilityChecker(
  transport: JoltTransport
): JoltCompatibilitySdk["checkCompatibility"] {
  let featureManifest: Promise<{
    response: AppApiFeatureManifestResponse;
    discovery: AppApiFeatureManifest["discovery"];
  }> | null = null;

  async function loadFeatureManifest(call?: CompatibilityCheckOptions) {
    const { refresh = false, ...transportOptions } = call ?? {};
    if (refresh || featureManifest === null) {
      const pending = ops
        .getAppApiFeatures(transport, transportOptions)
        .then((response) => ({ response, discovery: "advertised" as const }))
        .catch((error: unknown) => {
          if (error instanceof JoltApiError && error.status === 404) {
            return {
              response: { app_api: 1, features: {} },
              discovery: "legacy" as const,
            };
          }
          throw error;
        });
      featureManifest = pending;
      try {
        return await pending;
      } catch (error) {
        if (featureManifest === pending) featureManifest = null;
        throw error;
      }
    }
    return featureManifest;
  }

  return async (declaration, call) => {
    const { response, discovery } = await loadFeatureManifest(call);
    return evaluateCompatibility(declaration, response, discovery);
  };
}
