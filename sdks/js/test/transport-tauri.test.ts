import { beforeEach, describe, expect, it, vi } from "vitest";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  isTauri: vi.fn(() => true),
}));

vi.mock("@tauri-apps/api/core", () => tauri);

import { createJoltClient } from "../src/client.js";
import { JoltApiError, JoltTransportError } from "../src/errors.js";
import { TauriTransport } from "../src/transport-tauri.js";

beforeEach(() => {
  tauri.invoke.mockReset();
});

describe("TauriTransport compatibility", () => {
  it("produces the advertised compatibility result through the plugin command", async () => {
    tauri.invoke.mockResolvedValueOnce({
      app_api: 1,
      features: { "data.documents": 1 },
    });
    const client = createJoltClient({
      transport: new TauriTransport({ plugin: true }),
      getSessionToken: () => "",
    });

    await expect(
      client.checkCompatibility({
        appApi: 1,
        requiredFeatures: { "data.documents": 1 },
      })
    ).resolves.toMatchObject({ status: "compatible" });
    expect(tauri.invoke).toHaveBeenCalledWith("plugin:jolt|daemon_request", {
      basePath: "/app/v1",
      path: "/features",
      method: "GET",
      body: null,
      sessionToken: null,
    });
  });

  it("preserves a plugin HTTP 404 so the SDK selects the legacy baseline", async () => {
    tauri.invoke.mockRejectedValueOnce({
      kind: "api",
      message: "daemon returned 404 Not Found",
      status: 404,
      code: null,
      body: null,
    });
    const client = createJoltClient({
      transport: new TauriTransport({ plugin: true }),
      getSessionToken: () => "",
    });

    await expect(client.checkCompatibility({ appApi: 1 })).resolves.toMatchObject({
      status: "compatible",
      manifest: { appApi: 1, features: {}, discovery: "legacy" },
    });
  });

  it("recognizes the existing handwritten bridge's legacy 404 string", async () => {
    tauri.invoke.mockRejectedValueOnce("daemon returned 404 Not Found");
    const client = createJoltClient({
      transport: new TauriTransport(),
      getSessionToken: () => "",
    });

    await expect(client.checkCompatibility({ appApi: 1 })).resolves.toMatchObject({
      status: "compatible",
      manifest: { discovery: "legacy" },
    });
  });

  it("preserves plugin transport unavailability as a transport error", async () => {
    tauri.invoke.mockRejectedValueOnce({
      kind: "transport",
      message: "daemon request failed: connection refused",
      status: null,
      code: null,
      body: null,
    });
    const client = createJoltClient({
      transport: new TauriTransport({ plugin: true }),
      getSessionToken: () => "",
    });

    await expect(client.checkCompatibility({ appApi: 1 })).rejects.toBeInstanceOf(
      JoltTransportError
    );
  });

  it("classifies plugin configuration failures as transport errors", async () => {
    tauri.invoke.mockRejectedValueOnce({
      kind: "configuration",
      message: "invalid daemon request path",
      status: null,
      code: null,
      body: null,
    });
    const client = createJoltClient({
      transport: new TauriTransport({ plugin: true }),
      getSessionToken: () => "",
    });

    await expect(client.checkCompatibility({ appApi: 1 })).rejects.toBeInstanceOf(
      JoltTransportError
    );
  });

  it("preserves an object-shaped legacy rejection in the fallback error message", async () => {
    tauri.invoke.mockRejectedValueOnce({ reason: "bridge rejected request" });
    const client = createJoltClient({
      transport: new TauriTransport(),
      getSessionToken: () => "",
    });

    await expect(client.checkCompatibility({ appApi: 1 })).rejects.toMatchObject({
      constructor: JoltApiError,
      message: '{"reason":"bridge rejected request"}',
    });
  });
});
