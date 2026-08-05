/**
 * The transport seam: how SDK operations reach a Jolt daemon.
 *
 * The SDK core is transport-independent. A {@link JoltTransport} knows how to
 * carry a request to the daemon's app API (`/app/v1`) or trusted daemon API
 * (`/api/v1`); everything above it (operations, client, domain types) is the
 * same in the browser, Node.js, and Tauri.
 *
 * Ship transports:
 * - `@jolt/sdk/transport-http`: fetch-based, for browsers and Node.js.
 * - `@jolt/sdk/transport-tauri`: Tauri `invoke`-based, for desktop shells.
 * - `@jolt/sdk/testing`: an in-memory fake daemon for tests.
 *
 * @module
 */

/** Which daemon API surface a request targets. */
export type ApiBase = "app" | "daemon";

/**
 * Options accepted by every SDK operation.
 *
 * `signal` aborts the request; `timeoutMs` fails it with a
 * {@link JoltTransportError} after the given time. Both may be combined.
 */
export type CallOptions = {
  signal?: AbortSignal;
  timeoutMs?: number;
};

/** A JSON (or empty-body) request. */
export type TransportRequest = CallOptions & {
  method?: "GET" | "POST" | "DELETE";
  /** Bearer session token, when the endpoint needs one. */
  token?: string | null;
  /** JSON body; omitted for GET. */
  json?: unknown;
};

/** A multipart upload request (public publish and append). */
export type TransportUpload = CallOptions & {
  token: string;
  /** The logical Jolt path form field. */
  path: string;
  bytes: Uint8Array;
  fileName: string;
  mimeType: string;
};

/**
 * The single interface a transport implements.
 *
 * Implementations must throw {@link JoltApiError} for non-success daemon
 * responses and {@link JoltTransportError} for network-level failures, so
 * application error handling is uniform across runtimes.
 */
export interface JoltTransport {
  /** Send a JSON request to `base` + `path` and parse the JSON response. */
  request<T>(base: ApiBase, path: string, req?: TransportRequest): Promise<T>;
  /** Send a multipart file upload to `base` + `path` and parse the JSON response. */
  upload<T>(base: ApiBase, path: string, req: TransportUpload): Promise<T>;
}

/**
 * Combine a caller's abort signal with a timeout into one signal.
 *
 * @internal
 */
export function combinedSignal(options?: CallOptions): AbortSignal | undefined {
  const signals: AbortSignal[] = [];
  if (options?.signal) signals.push(options.signal);
  if (options?.timeoutMs !== undefined) signals.push(AbortSignal.timeout(options.timeoutMs));
  if (signals.length === 0) return undefined;
  if (signals.length === 1) return signals[0];
  return AbortSignal.any(signals);
}
