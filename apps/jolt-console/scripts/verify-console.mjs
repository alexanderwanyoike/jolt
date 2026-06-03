import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;

const files = {
  packageJson: readFileSync(join(root, "package.json"), "utf8"),
  main: readFileSync(join(root, "src/main.tsx"), "utf8"),
  app: readFileSync(join(root, "src/app/App.tsx"), "utf8"),
  navigation: readFileSync(join(root, "src/app/navigation.ts"), "utf8"),
  daemonClient: readFileSync(join(root, "src/daemon/client.ts"), "utf8"),
  appsPage: readFileSync(join(root, "src/sections/AppsPage.tsx"), "utf8"),
  styles: readFileSync(join(root, "src/styles.css"), "utf8"),
  tauriConfig: readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"),
  tauriLib: readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8")
};

const requiredSections = [
  "Overview",
  "Identity",
  "Apps",
  "Network",
  "Relays",
  "Published",
  "Cache",
  "Settings",
  "Diagnostics"
];

for (const marker of requiredSections) {
  if (!files.navigation.includes(marker)) {
    throw new Error(`Missing console section in navigation: ${marker}`);
  }
}

for (const marker of ["daemon_get", "/api/v1/status", "/api/v1/cache/stats", "/api/v1/published"]) {
  if (!files.daemonClient.includes(marker)) {
    throw new Error(`Missing daemon marker in src/daemon/client.ts: ${marker}`);
  }
}

if (!files.app.includes("HashRouter")) {
  throw new Error("Console must use Tauri-safe hash routing");
}

if (!files.main.includes("createRoot") || !files.packageJson.includes("react-router-dom")) {
  throw new Error("Console must be wired as a React app");
}

if (!files.appsPage.includes("/admin/v1/app-requests") || !files.appsPage.includes("/admin/v1/app-sessions")) {
  throw new Error("Apps section must reserve the app permission API surface");
}

for (const marker of ["Jolt Console"]) {
  if (!files.tauriConfig.includes(marker) && !files.tauriLib.includes(marker)) {
    throw new Error(`Missing Tauri marker: ${marker}`);
  }
}

if (!files.packageJson.includes("@jolt/console")) {
  throw new Error("Missing Console package metadata");
}

for (const marker of ["console-shell", "sidebar", "section-panel"]) {
  if (!files.styles.includes(marker)) {
    throw new Error(`Missing layout marker in src/styles.css: ${marker}`);
  }
}

console.log("Jolt Console scaffold verified");
