import { createJoltClient } from "jolt-sdk";
import { TauriTransport } from "jolt-sdk/transport-tauri";

import {
  capabilitiesFor,
  checkChirpCompatibility,
  type ChirpCompatibility,
} from "./compatibility";

const TOKEN_KEY = "chirp.session-token";
let token = localStorage.getItem(TOKEN_KEY) ?? "";

export const jolt = createJoltClient({
  transport: new TauriTransport({ plugin: true }),
  getSessionToken: () => token,
});

const HOME_RELAY_CAPABILITY = "pin:own:/chirp/*";

export type ChirpConnection = Extract<ChirpCompatibility, { status: "ready" }> & {
  identity: string;
  homeRelayAuthorized: boolean;
};

export type ChirpConnectResult =
  | ChirpConnection
  | Exclude<ChirpCompatibility, { status: "ready" }>;

function connected(
  identity: string,
  compatibility: Extract<ChirpCompatibility, { status: "ready" }>,
  grantedCapabilities: string[]
): ChirpConnection {
  return {
    ...compatibility,
    identity,
    homeRelayAuthorized: grantedCapabilities.includes(HOME_RELAY_CAPABILITY),
  };
}

export async function connect(): Promise<ChirpConnectResult> {
  const compatibility = await checkChirpCompatibility(jolt);
  if (compatibility.status !== "ready") return compatibility;

  const requestedCapabilities = capabilitiesFor(compatibility);
  const status = await jolt.getStatus();
  if (token) {
    try {
      const session = await jolt.getCurrentSession();
      if (session.status === "active") {
        return connected(
          status.identity_address,
          compatibility,
          session.granted_capabilities
        );
      }
    } catch {
      token = ""; // stored token was revoked or expired; ask again
    }
  }

  const request = await jolt.requestSession({
    appId: "chirp.example",
    appName: "Chirp",
    appOrigin: "tauri://chirp.example",
    identity: status.identity_address,
    capabilities: requestedCapabilities,
  });

  for (;;) {
    const s = await jolt.getSessionRequestStatus(request.request_id);
    if (s.status === "rejected") {
      throw new Error("Chirp's session request was rejected in Jolt Console.");
    }
    if (s.session_token) {
      token = s.session_token;
      localStorage.setItem(TOKEN_KEY, token);
      return connected(status.identity_address, compatibility, s.capabilities);
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
}
