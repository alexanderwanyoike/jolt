import {
  apiErrorMessage,
  JoltTransportError,
  type AppCompatibilityDeclaration,
  type JoltCompatibilitySdk,
} from "jolt-sdk";

export const CHIRP_HOME_RELAY_FEATURE = "availability.home-relay-pin";

/** Compatibility belongs to this Chirp release, not to the Jolt daemon. */
export const CHIRP_COMPATIBILITY = {
  appApi: 1,
  requiredFeatures: {},
  optionalFeatures: { [CHIRP_HOME_RELAY_FEATURE]: 1 },
} as const satisfies AppCompatibilityDeclaration;

export type ChirpCompatibility =
  | {
      status: "ready";
      discovery: "advertised" | "legacy";
      homeRelayAvailability: "available" | "hidden";
    }
  | {
      status: "incompatible";
      requiredAppApi: number;
      availableAppApi: number | null;
    }
  | { status: "unavailable"; message: string };

const BASE_CAPABILITIES = [
  "publish:/chirp/*", // write posts and the follow list
  "publish:encrypted:/chirp/*", // keep the sender's copy of ingress objects
  "resolve:public", // resolve .jolt addresses
  "fetch:public", // fetch content by content id
  "enumerate:self:/chirp/*", // list our own append records
  "enumerate:any:/chirp/*", // list other identities' /chirp/ records
  "ingress:send", // deliver follow requests
  "ingress:read", // list and open our pending inbox
  "ingress:decide", // accept or reject inbox envelopes
];

/** Request authorization separately, and only for behavior this daemon supports. */
export function capabilitiesFor(compatibility: ChirpCompatibility): string[] {
  if (
    compatibility.status === "ready" &&
    compatibility.homeRelayAvailability === "available"
  ) {
    return [...BASE_CAPABILITIES, "pin:own:/chirp/*"];
  }
  return [...BASE_CAPABILITIES];
}

/** Evaluate compatibility once when Chirp establishes its daemon connection. */
export async function checkChirpCompatibility(
  jolt: JoltCompatibilitySdk
): Promise<ChirpCompatibility> {
  let result;
  try {
    result = await jolt.checkCompatibility(CHIRP_COMPATIBILITY);
  } catch (error) {
    if (error instanceof JoltTransportError) {
      return { status: "unavailable", message: apiErrorMessage(error) };
    }
    throw error;
  }

  if (result.status === "incompatible") {
    return {
      status: "incompatible",
      requiredAppApi: result.appApi.requiredLevel,
      availableAppApi: result.appApi.availableLevel,
    };
  }

  return {
    status: "ready",
    discovery: result.manifest.discovery,
    homeRelayAvailability: result.optionalFeatures[CHIRP_HOME_RELAY_FEATURE]?.supported
      ? "available"
      : "hidden",
  };
}
