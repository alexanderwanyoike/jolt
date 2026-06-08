import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";

export type ConsoleUpdateAvailable = {
  available: true;
  version: string;
  currentVersion: string;
  notes?: string;
  date?: string;
};

export type ConsoleUpdateUnavailable = {
  available: false;
  currentVersion?: string;
};

export type ConsoleUpdateCheck = ConsoleUpdateAvailable | ConsoleUpdateUnavailable;

export type ConsoleUpdateClient = {
  check(): Promise<ConsoleUpdateCheck>;
  installAndRelaunch(): Promise<void>;
};

let pendingUpdate: Update | null = null;

export const tauriConsoleUpdateClient: ConsoleUpdateClient = {
  async check() {
    const update = await check();
    pendingUpdate = update ?? null;

    if (!update) {
      return { available: false };
    }

    return {
      available: true,
      version: update.version,
      currentVersion: update.currentVersion,
      notes: update.body,
      date: update.date
    };
  },

  async installAndRelaunch() {
    const update = pendingUpdate ?? (await check());
    if (!update) {
      throw new Error("No Console update is pending");
    }

    pendingUpdate = null;
    await update.downloadAndInstall();
    await relaunch();
  }
};

