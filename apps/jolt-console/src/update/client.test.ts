import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { tauriConsoleUpdateClient } from "./client";

vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn()
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => "appimage")
}));

vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn()
}));

describe("tauriConsoleUpdateClient", () => {
  beforeEach(() => {
    vi.mocked(check).mockReset();
    vi.mocked(relaunch).mockReset();
  });

  it("never consults the updater for a package install", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    vi.mocked(invoke).mockResolvedValueOnce("deb");

    await expect(tauriConsoleUpdateClient.check()).resolves.toEqual({
      available: false,
      managedByPackage: true
    });
    expect(check).not.toHaveBeenCalled();
  });

  it("reports no update when the Tauri updater returns none", async () => {
    vi.mocked(check).mockResolvedValueOnce(null);

    await expect(tauriConsoleUpdateClient.check()).resolves.toEqual({ available: false });
  });

  it("installs the pending signed update and relaunches Console", async () => {
    const update = {
      version: "0.2.0",
      currentVersion: "0.1.0",
      body: "Release notes",
      date: "2026-06-08T12:00:00Z",
      downloadAndInstall: vi.fn(async () => undefined)
    };
    vi.mocked(check).mockResolvedValueOnce(update);

    await expect(tauriConsoleUpdateClient.check()).resolves.toEqual({
      available: true,
      version: "0.2.0",
      currentVersion: "0.1.0",
      notes: "Release notes",
      date: "2026-06-08T12:00:00Z"
    });

    await tauriConsoleUpdateClient.installAndRelaunch();

    expect(update.downloadAndInstall).toHaveBeenCalledOnce();
    expect(relaunch).toHaveBeenCalledOnce();
  });
});

