/**
 * Typed errors thrown by the SDK.
 *
 * Every failure surfaced by a transport is one of these two classes, so
 * applications can distinguish "the daemon answered with an error" from "the
 * daemon could not be reached" without string matching.
 *
 * @module
 */

/**
 * The daemon (or its dev proxy) answered with a non-success status.
 *
 * The daemon's JSON error body, when present, is preserved on {@link body} and
 * its `error` field becomes the message.
 */
export class JoltApiError extends Error {
  /** HTTP status code of the response, when the transport had one. */
  readonly status?: number;
  /** Machine-readable error code from the daemon body (e.g. `app_session_unauthorized`), when present. */
  readonly code?: string;
  /** The raw parsed error body, for callers that need more than the message. */
  readonly body?: unknown;

  constructor(message: string, options: { status?: number; code?: string; body?: unknown } = {}) {
    super(message);
    this.name = "JoltApiError";
    this.status = options.status;
    this.code = options.code;
    this.body = options.body;
  }
}

/**
 * The daemon could not be reached at all: connection refused, DNS failure,
 * aborted request, or timeout. The {@link cause} carries the underlying error.
 */
export class JoltTransportError extends Error {
  constructor(message: string, options: { cause?: unknown } = {}) {
    super(message);
    this.name = "JoltTransportError";
    this.cause = options.cause;
  }
}

/**
 * Map any error thrown by the SDK to a short human-readable message suitable
 * for an app's error banner. Never throws.
 */
export function apiErrorMessage(error: unknown): string {
  if (error instanceof JoltTransportError) {
    return "Cannot reach the Jolt daemon. Start it on the configured API port and try again.";
  }
  if (error instanceof JoltApiError) {
    if (error.status === 500 || error.status === 502) {
      return "The Jolt daemon returned an internal error. Check the daemon log.";
    }
    return error.message;
  }
  if (error instanceof TypeError) {
    return "Cannot reach the dev proxy or Jolt daemon.";
  }
  if (error instanceof Error) {
    return error.message;
  }
  return String(error);
}
