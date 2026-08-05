import { afterEach, describe, expect, it, vi } from "vitest";

import { JoltApiError, JoltTransportError } from "../src/errors.js";
import { HttpTransport } from "../src/transport-http.js";

afterEach(() => {
  vi.unstubAllGlobals();
});

function stubFetch(handler: (url: string, init?: RequestInit) => Response | Promise<Response>) {
  const calls: Array<{ url: string; init?: RequestInit }> = [];
  vi.stubGlobal("fetch", async (url: string, init?: RequestInit) => {
    calls.push({ url, init });
    return handler(url, init);
  });
  return calls;
}

describe("HttpTransport", () => {
  it("derives app and daemon bases from the daemon url", async () => {
    const calls = stubFetch(() => Response.json({ ok: true }));
    const transport = new HttpTransport({ daemonUrl: "http://127.0.0.1:9999/" });

    await transport.request("app", "/published", { token: "t" });
    await transport.request("daemon", "/status");

    expect(calls[0]!.url).toBe("http://127.0.0.1:9999/app/v1/published");
    expect(calls[1]!.url).toBe("http://127.0.0.1:9999/api/v1/status");
    const headers = calls[0]!.init?.headers as Record<string, string>;
    expect(headers.Authorization).toBe("Bearer t");
  });

  it("viteProxy preset targets the proxy base paths", async () => {
    const calls = stubFetch(() => Response.json({}));
    const transport = HttpTransport.viteProxy();

    await transport.request("app", "/session", { token: "t" });
    await transport.request("daemon", "/status");

    expect(calls[0]!.url).toBe("/jolt-api/session");
    expect(calls[1]!.url).toBe("/jolt-daemon/status");
  });

  it("maps daemon error bodies to JoltApiError with code and status", async () => {
    stubFetch(() =>
      new Response(JSON.stringify({ code: "app_session_unauthorized", error: "bad token" }), {
        status: 401,
        headers: { "content-type": "application/json" },
      })
    );
    const transport = new HttpTransport();

    const failure = transport.request("app", "/published", { token: "t" });
    await expect(failure).rejects.toBeInstanceOf(JoltApiError);
    await expect(failure).rejects.toMatchObject({
      status: 401,
      code: "app_session_unauthorized",
      message: "bad token",
    });
  });

  it("maps network failures to JoltTransportError", async () => {
    vi.stubGlobal("fetch", async () => {
      throw new TypeError("fetch failed");
    });
    const transport = new HttpTransport();

    await expect(transport.request("daemon", "/status")).rejects.toBeInstanceOf(
      JoltTransportError
    );
  });

  it("aborts via timeout", async () => {
    vi.stubGlobal(
      "fetch",
      (_url: string, init?: RequestInit) =>
        new Promise<Response>((_resolve, reject) => {
          init?.signal?.addEventListener("abort", () => reject(init.signal!.reason));
        })
    );
    const transport = new HttpTransport();

    await expect(
      transport.request("daemon", "/status", { timeoutMs: 20 })
    ).rejects.toBeInstanceOf(JoltTransportError);
  });
});
