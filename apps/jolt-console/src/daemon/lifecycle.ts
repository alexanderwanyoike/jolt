import { invoke } from "@tauri-apps/api/core";

export type DaemonReachability = "healthy" | "unhealthy" | "unavailable";
export type DaemonOwnership = "none" | "console" | "external";

export type DaemonLifecycleState = {
  daemon_url: string;
  reachability: DaemonReachability;
  ownership: DaemonOwnership;
  pid?: number | null;
  message: string;
  last_error?: string | null;
  log_tail?: string[];
};

export type DaemonLifecycleClient = {
  status(): Promise<DaemonLifecycleState>;
  start(): Promise<DaemonLifecycleState>;
  stop(): Promise<DaemonLifecycleState>;
  restart(): Promise<DaemonLifecycleState>;
};

export const tauriDaemonLifecycleClient: DaemonLifecycleClient = {
  status() {
    return invoke<DaemonLifecycleState>("daemon_lifecycle_status");
  },
  start() {
    return invoke<DaemonLifecycleState>("daemon_lifecycle_start");
  },
  stop() {
    return invoke<DaemonLifecycleState>("daemon_lifecycle_stop");
  },
  restart() {
    return invoke<DaemonLifecycleState>("daemon_lifecycle_restart");
  }
};
