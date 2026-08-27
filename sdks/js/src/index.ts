/**
 * `jolt-sdk`: the TypeScript SDK for building applications on the Jolt
 * network.
 *
 * Layers, top to bottom:
 *
 * 1. **Client** ({@link createJoltClient}): domain-shaped operations with
 *    tolerant reads. What applications program against.
 * 2. **Operations**: one typed function per stable daemon endpoint, for the
 *    rare call the client does not wrap.
 * 3. **Transports**: how requests reach the daemon. Import one from
 *    `jolt-sdk/transport-http` or `jolt-sdk/transport-tauri`, or the fake
 *    from `jolt-sdk/testing`.
 *
 * Quick start (browser or Node.js):
 *
 * ```ts
 * import { createJoltClient } from "jolt-sdk";
 * import { HttpTransport } from "jolt-sdk/transport-http";
 *
 * let token = "";
 * const jolt = createJoltClient({
 *   transport: new HttpTransport({ daemonUrl: "http://127.0.0.1:9862" }),
 *   getSessionToken: () => token,
 * });
 *
 * const request = await jolt.requestSession({
 *   appId: "myapp.local",
 *   appName: "My App",
 *   appOrigin: "http://127.0.0.1:5173",
 *   identity: (await jolt.getStatus()).identity_address,
 *   capabilities: ["publish:/myapp/*", "resolve:public", "fetch:public"],
 * });
 * // ...poll jolt.getSessionRequestStatus(request.request_id) until it
 * // carries a session_token (the user approves in Jolt Console), then:
 * await jolt.publishJson("/myapp/hello", { hello: "world" });
 * ```
 *
 * @module
 */

export {
  createJoltClient,
  makeId,
  referenceKey,
  referenceTarget,
} from "./client.js";
export type {
  Decoder,
  EnumeratedRecord,
  HomeRelayPinResult,
  JoltAppendSdk,
  JoltAvailabilitySdk,
  JoltClient,
  JoltClientOptions,
  JoltEncryptedSdk,
  JoltIngressSdk,
  JoltSdk,
  JoltSessionSdk,
  OpenEncryptedResult,
  PublishResult,
  RecordDeletedResult,
  RecordMissingResult,
  RecordMutationContext,
  RecordPresentResult,
  RecordReadResult,
  Reference,
  ResolvedReference,
  Versioned,
} from "./client.js";

export type {
  AppApiFeatureManifest,
  AppCompatibilityDeclaration,
  AppCompatibilityDeclarationWire,
  AppCompatibilityResult,
  CompatibilityCheckOptions,
  ContractLevelCheck,
  JoltCompatibilitySdk,
} from "./compatibility.js";
export { decodeAppCompatibilityDeclaration } from "./compatibility.js";

export {
  apiErrorMessage,
  isContentUnavailableError,
  isJoltUnavailableError,
  JoltApiError,
  JoltTransportError,
} from "./errors.js";

export type {
  ApiBase,
  CallOptions,
  JoltTransport,
  TransportRequest,
  TransportUpload,
} from "./transport.js";

export * as operations from "./operations.js";
export type { SessionRequest } from "./operations.js";

export type {
  AppendRecordInfo,
  AppApiFeatureManifestResponse,
  AppSessionRequestResponse,
  AppSessionStatus,
  AppSessionStatusResponse,
  CurrentAppSession,
  DecryptedEncryptedObject,
  DecryptedIngress,
  EncryptedPublishResponse,
  FetchResult,
  HomeRelayPinResponse,
  IngressRecord,
  LocalRecordReadResponse,
  LocalRecordUpdateResponse,
  NodeStatus,
  OpenedEncryptedObject,
  PublishedContent,
  PublishResponse,
  ResolveResponse,
} from "./wire.js";
