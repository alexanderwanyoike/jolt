import { readFileSync } from "node:fs";

const files = {
  workflow: readFileSync(".github/workflows/package-jolt-console.yml", "utf8"),
  installer: readFileSync("scripts/install-jolt-console.sh", "utf8"),
  packageScript: readFileSync("scripts/package-jolt-console.sh", "utf8"),
  readme: readFileSync("README.md", "utf8"),
  card: readFileSync("docs/cards/077-jolt-distribution-v0.md", "utf8")
};

const requiredMarkers = {
  workflow: [
    "Package Jolt Console",
    "scripts/package-jolt-console.sh",
    "jolt-console-x86_64.AppImage",
    "actions/upload-artifact",
    "softprops/action-gh-release",
    "refs/tags/"
  ],
  installer: [
    "JOLT_VERSION",
    "JOLT_INSTALL_DIR",
    "jolt-console-x86_64.AppImage",
    "releases/latest",
    "releases/download",
    "--check",
    "--update",
    "run_with_retries",
    ".local/bin"
  ],
  packageScript: [
    "target/release/bundle/appimage",
    "tauri build",
    "Prefetching Tauri AppImage helper binaries",
    "linuxdeploy-x86_64.AppImage"
  ],
  readme: [
    "curl -fsSL",
    "scripts/install-jolt-console.sh",
    "JOLT_VERSION=",
    "jolt-console --appimage-help"
  ],
  card: ["GitHub Actions", "jolt-console-x86_64.AppImage", "install-jolt-console.sh"]
};

for (const [fileName, markers] of Object.entries(requiredMarkers)) {
  for (const marker of markers) {
    if (!files[fileName].includes(marker)) {
      throw new Error(`Missing distribution marker in ${fileName}: ${marker}`);
    }
  }
}

console.log("Jolt distribution contract verified");
