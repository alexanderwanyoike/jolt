/**
 * HTTP transport: reach a Jolt daemon over `fetch`.
 *
 * Works in browsers and Node.js 18+. Point it at the daemon directly
 * (`new HttpTransport({ daemonUrl: "http://127.0.0.1:9862" })`) or, for
 * browser dev servers that proxy the daemon to dodge CORS, at proxy base
 * paths (`HttpTransport.viteProxy()` uses `/jolt-api` and `/jolt-daemon`,
 * the convention Spoke's vite config established).
 *
 * @module
 */

import { JoltApiError, JoltTransportError } from "./errors.js";
import type {
  ApiBase,
  JoltTransport,
  TransportRequest,
  TransportUpload,
} from "./transport.js";
import { combinedSignal } from "./transport.js";

/** Configuration for {@link HttpTransport}. */
export type HttpTransportOptions = {
  /**
   * Base URL of the daemon, e.g. `http://127.0.0.1:9862`. The transport
   * derives `/app/v1` and `/api/v1` from it.
   */
  daemonUrl?: string;
  /**
   * Explicit base paths/URLs per API surface, overriding `daemonUrl`.
   * Useful behind dev-server proxies.
   */
  bases?: { app: string; daemon: string };
  /** Default timeout applied when a call passes none. */
  defaultTimeoutMs?: number;
};

const DEFAULT_DAEMON_URL = "http://127.0.0.1:9862";

/** {@inheritDoc HttpTransportOptions} */
export class HttpTransport implements JoltTransport {
  private readonly bases: { app: string; daemon: string };
  private readonly defaultTimeoutMs?: number;

  constructor(options: HttpTransportOptions = {}) {
    if (options.bases) {
      this.bases = options.bases;
    } else {
      const url = (options.daemonUrl ?? DEFAULT_DAEMON_URL).replace(/\/+$/, "");
      this.bases = { app: `${url}/app/v1`, daemon: `${url}/api/v1` };
    }
    this.defaultTimeoutMs = options.defaultTimeoutMs;
  }

  /**
   * The browser dev-server preset: requests go to the `/jolt-api` (app API)
   * and `/jolt-daemon` (daemon API) proxy paths on the current origin.
   */
  static viteProxy(options: Omit<HttpTransportOptions, "bases" | "daemonUrl"> = {}): HttpTransport {
    return new HttpTransport({ ...options, bases: { app: "/jolt-api", daemon: "/jolt-daemon" } });
  }

  async request<T>(base: ApiBase, path: string, req: TransportRequest = {}): Promise<T> {
    const headers: Record<string, string> = {};
    if (req.token) headers.Authorization = `Bearer ${req.token}`;
    let body: string | undefined;
    if (req.json !== undefined) {
      headers["Content-Type"] = "application/json";
      body = JSON.stringify(req.json);
    }
    return this.send<T>(base, path, {
      method: req.method ?? (req.json !== undefined ? "POST" : "GET"),
      headers,
      body,
      signal: this.signalFor(req),
    });
  }

  async upload<T>(base: ApiBase, path: string, req: TransportUpload): Promise<T> {
    const form = new FormData();
    form.append("file", new Blob([req.bytes as BlobPart], { type: req.mimeType }), req.fileName);
    form.append("path", req.path);
    return this.send<T>(base, path, {
      method: "POST",
      headers: { Authorization: `Bearer ${req.token}` },
      body: form,
      signal: this.signalFor(req),
    });
  }

  private signalFor(req: { signal?: AbortSignal; timeoutMs?: number }): AbortSignal | undefined {
    return combinedSignal({
      signal: req.signal,
      timeoutMs: req.timeoutMs ?? this.defaultTimeoutMs,
    });
  }

  private async send<T>(
    base: ApiBase,
    path: string,
    init: RequestInit & { signal?: AbortSignal }
  ): Promise<T> {
    let response: Response;
    try {
      response = await fetch(`${this.bases[base]}${path}`, init);
    } catch (cause) {
      throw new JoltTransportError(`Jolt daemon request failed: ${String(cause)}`, { cause });
    }

    const contentType = response.headers.get("content-type") || "";
    const parsed = contentType.includes("application/json")
      ? await response.json().catch(() => null)
      : await response.text();

    if (!response.ok) {
      const bodyError =
        parsed && typeof parsed === "object" && "error" in parsed
          ? String((parsed as { error: unknown }).error)
          : typeof parsed === "string" && parsed.trim()
            ? parsed
            : `HTTP ${response.status}`;
      const code =
        parsed && typeof parsed === "object" && "code" in parsed
          ? String((parsed as { code: unknown }).code)
          : undefined;
      throw new JoltApiError(bodyError, { status: response.status, code, body: parsed });
    }

    return parsed as T;
  }
}
