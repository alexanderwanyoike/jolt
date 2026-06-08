import { readFileSync } from "node:fs";
import { join } from "node:path";

const root = new URL("..", import.meta.url).pathname;

const files = {
  packageJson: readFileSync(join(root, "package.json"), "utf8"),
  packageScript: readFileSync(join(root, "../../scripts/package-jolt-console.sh"), "utf8"),
  main: readFileSync(join(root, "src/main.tsx"), "utf8"),
  app: readFileSync(join(root, "src/app/App.tsx"), "utf8"),
  shell: readFileSync(join(root, "src/components/ConsoleShell.tsx"), "utf8"),
  navigation: readFileSync(join(root, "src/app/navigation.ts"), "utf8"),
  daemonClient: readFileSync(join(root, "src/daemon/client.ts"), "utf8"),
  appsPage: readFileSync(join(root, "src/sections/AppsPage.tsx"), "utf8"),
  settingsPage: readFileSync(join(root, "src/sections/SettingsPage.tsx"), "utf8"),
  updateClient: readFileSync(join(root, "src/update/client.ts"), "utf8"),
  styles: readFileSync(join(root, "src/styles.css"), "utf8"),
  tauriConfig: readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"),
  tauriCapabilities: readFileSync(join(root, "src-tauri/capabilities/default.json"), "utf8"),
  tauriLib: readFileSync(join(root, "src-tauri/src/lib.rs"), "utf8"),
  distributionVerifier: readFileSync(join(root, "../../scripts/verify-distribution.mjs"), "utf8")
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

const tauriConfig = JSON.parse(files.tauriConfig);
if (tauriConfig.bundle?.active !== true) {
  throw new Error("Tauri bundle must be enabled for v0 distribution");
}

if (!Array.isArray(tauriConfig.bundle?.targets) || !tauriConfig.bundle.targets.includes("appimage")) {
  throw new Error("Tauri bundle must include a Linux AppImage target");
}

if (!Array.isArray(tauriConfig.bundle?.externalBin) || !tauriConfig.bundle.externalBin.includes("binaries/jolt")) {
  throw new Error("Tauri bundle must declare the jolt daemon sidecar");
}

if (!tauriConfig.plugins?.updater?.pubkey || !tauriConfig.plugins?.updater?.endpoints?.length) {
  throw new Error("Tauri updater must be configured with a public key and endpoint");
}

for (const marker of ["tauri_plugin_updater", "tauri_plugin_process"]) {
  if (!files.tauriLib.includes(marker)) {
    throw new Error(`Missing native updater plugin marker: ${marker}`);
  }
}

for (const marker of ["updater:default", "process:default"]) {
  if (!files.tauriCapabilities.includes(marker)) {
    throw new Error(`Missing updater capability permission: ${marker}`);
  }
}

for (const marker of ["@tauri-apps/plugin-updater", "@tauri-apps/plugin-process"]) {
  if (!files.packageJson.includes(marker)) {
    throw new Error(`Missing updater package dependency: ${marker}`);
  }
}

for (const marker of ["check()", "downloadAndInstall", "relaunch"]) {
  if (!files.updateClient.includes(marker)) {
    throw new Error(`Missing updater client marker: ${marker}`);
  }
}

if (!files.settingsPage.includes("Install and restart") || !files.shell.includes("Update ")) {
  throw new Error("Console must expose update availability and install controls");
}

if (!files.packageJson.includes("@jolt/console")) {
  throw new Error("Missing Console package metadata");
}

if (
  !files.packageJson.includes("package:linux") ||
  !files.packageScript.includes("TAURI_BUILD_ARGS") ||
  !files.distributionVerifier.includes("jolt-console-x86_64.AppImage")
) {
  throw new Error("Console package metadata must expose the Linux packaging script");
}

for (const marker of ["console-shell", "sidebar", "section-panel"]) {
  if (!files.styles.includes(marker)) {
    throw new Error(`Missing layout marker in src/styles.css: ${marker}`);
  }
}

console.log("Jolt Console scaffold verified");
