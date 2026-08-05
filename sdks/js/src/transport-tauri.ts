/**
 * Tauri transport: reach the Jolt daemon through a desktop shell's Rust
 * commands instead of direct HTTP, so the webview never needs network access
 * to the daemon.
 *
 * The host application must expose these Tauri commands (the contract Spoke
 * established):
 *
 * ```rust
 * #[tauri::command]
 * async fn daemon_request(base_path: String, path: String, method: String,
 *                         body: Option<Value>, session_token: Option<String>)
 *                         -> Result<Value, String>;
 * #[tauri::command]
 * async fn daemon_publish_bytes(session_token: String, path: String,
 *                               bytes: Vec<u8>, file_name: String,
 *                               mime_type: String) -> Result<Value, String>;
 * #[tauri::command]
 * async fn daemon_append(session_token: String, path: String, bytes: Vec<u8>,
 *                        file_name: String, mime_type: String)
 *                        -> Result<Value, String>;
 * ```
 *
 * See the app development guide for the full Rust implementation to copy.
 *
 * @module
 */

import { invoke, isTauri } from "@tauri-apps/api/core";

import { JoltApiError } from "./errors.js";
import type {
  ApiBase,
  JoltTransport,
  TransportRequest,
  TransportUpload,
} from "./transport.js";

const BASE_PATHS: Record<ApiBase, string> = { app: "/app/v1", daemon: "/api/v1" };

/**
 * True when running inside a Tauri webview. Use this to choose between
 * {@link TauriTransport} and an HTTP transport in apps that also run in a
 * plain browser during development.
 */
export function isTauriRuntime(): boolean {
  const internals =
    typeof window === "undefined"
      ? null
      : (window as typeof window & { __TAURI_INTERNALS__?: { invoke?: unknown } })
          .__TAURI_INTERNALS__;
  return isTauri() || typeof internals?.invoke === "function";
}

/** Configuration for {@link TauriTransport}. */
export type TauriTransportOptions = {
  /**
   * Invoke the commands provided by the `tauri-plugin-jolt` Rust plugin
   * (`plugin:jolt|daemon_request` etc.) instead of app-defined commands.
   * Recommended: add `tauri_plugin_jolt::init()` to your Tauri builder and
   * the `jolt:default` capability, and no hand-written Rust proxy is needed.
   */
  plugin?: boolean;
};

/** {@inheritDoc isTauriRuntime} */
export class TauriTransport implements JoltTransport {
  private readonly prefix: string;

  constructor(options: TauriTransportOptions = {}) {
    this.prefix = options.plugin ? "plugin:jolt|" : "";
  }

  async request<T>(base: ApiBase, path: string, req: TransportRequest = {}): Promise<T> {
    try {
      return await invoke<T>(`${this.prefix}daemon_request`, {
        basePath: BASE_PATHS[base],
        path,
        method: req.method ?? (req.json !== undefined ? "POST" : "GET"),
        body: req.json ?? null,
        sessionToken: req.token ?? null,
      });
    } catch (error) {
      throw toApiError(error);
    }
  }

  async upload<T>(base: ApiBase, path: string, req: TransportUpload): Promise<T> {
    const command = path === "/append" ? "daemon_append" : "daemon_publish_bytes";
    try {
      return await invoke<T>(`${this.prefix}${command}`, {
        sessionToken: req.token,
        path: req.path,
        bytes: Array.from(req.bytes),
        fileName: req.fileName,
        mimeType: req.mimeType,
      });
    } catch (error) {
      throw toApiError(error);
    }
  }
}

/** Tauri commands reject with a plain string; normalize to the SDK error type. */
function toApiError(error: unknown): JoltApiError {
  return new JoltApiError(typeof error === "string" ? error : String(error), { body: error });
}
