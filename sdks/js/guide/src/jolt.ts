import { createJoltClient } from "jolt-sdk";
import { TauriTransport } from "jolt-sdk/transport-tauri";

const TOKEN_KEY = "chirp.session-token";
let token = localStorage.getItem(TOKEN_KEY) ?? "";

export const jolt = createJoltClient({
  transport: new TauriTransport({ plugin: true }),
  getSessionToken: () => token,
});

export const CAPABILITIES = [
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

export async function connect(): Promise<string> {
  const status = await jolt.getStatus();
  if (token) {
    try {
      const session = await jolt.getCurrentSession();
      if (session.status === "active") return status.identity_address;
    } catch {
      token = ""; // stored token was revoked or expired; ask again
    }
  }

  const request = await jolt.requestSession({
    appId: "chirp.example",
    appName: "Chirp",
    appOrigin: "tauri://chirp.example",
    identity: status.identity_address,
    capabilities: CAPABILITIES,
  });

  for (;;) {
    const s = await jolt.getSessionRequestStatus(request.request_id);
    if (s.status === "rejected") {
      throw new Error("Chirp's session request was rejected in Jolt Console.");
    }
    if (s.session_token) {
      token = s.session_token;
      localStorage.setItem(TOKEN_KEY, token);
      return status.identity_address;
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
}
