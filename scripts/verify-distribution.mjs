import { readFileSync } from "node:fs";

const files = {
  workflow: readFileSync(".github/workflows/package-jolt-console.yml", "utf8"),
  installer: readFileSync("scripts/install-jolt-console.sh", "utf8"),
  packageScript: readFileSync("scripts/package-jolt-console.sh", "utf8"),
  updateManifest: readFileSync("scripts/write-jolt-console-update-manifest.mjs", "utf8"),
  readme: readFileSync("README.md", "utf8"),
  distributionCard: readFileSync("docs/cards/077-jolt-distribution-v0.md", "utf8"),
  installCliCard: readFileSync("docs/cards/084-install-jolt-cli-with-console.md", "utf8")
};

const requiredMarkers = {
  workflow: [
    "Package Jolt Console",
    "scripts/package-jolt-console.sh",
    "jolt-console-x86_64.AppImage",
    "actions/upload-artifact",
    "softprops/action-gh-release",
    "refs/tags/",
    "jolt-console-x86_64.AppImage.sig",
    "latest.json",
    "write-jolt-console-update-manifest.mjs",
    "jolt-linux-x86_64",
    "jolt-linux-x86_64.sha256",
    "target/release/jolt"
  ],
  installer: [
    "JOLT_VERSION",
    "JOLT_INSTALL_DIR",
    "jolt-console-x86_64.AppImage",
    "jolt-linux-x86_64",
    "JOLT_CLI_ASSET_NAME",
    "JOLT_CLI_BIN_NAME",
    "--cli-only",
    "--console-only",
    "releases/latest",
    "releases/download",
    "--check",
    "--update",
    "run_with_retries",
    ".local/bin"
  ],
  packageScript: [
    "target/release/bundle/appimage",
    "TAURI_BUILD_ARGS",
    "Prefetching Tauri AppImage helper binaries",
    "linuxdeploy-x86_64.AppImage",
    "JOLT_CREATE_UPDATER_ARTIFACTS",
    "createUpdaterArtifacts"
  ],
  updateManifest: [
    "latest.json",
    "linux-x86_64",
    "signature",
    "jolt-console-x86_64.AppImage"
  ],
  readme: [
    "curl -fsSL",
    "scripts/install-jolt-console.sh",
    "JOLT_VERSION=",
    "jolt-console --appimage-help",
    "jolt --version",
    "--cli-only"
  ],
  distributionCard: ["GitHub Actions", "jolt-console-x86_64.AppImage", "install-jolt-console.sh"],
  installCliCard: ["jolt-linux-x86_64", "jolt-console", "jolt", "--cli-only"]
};

for (const [fileName, markers] of Object.entries(requiredMarkers)) {
  for (const marker of markers) {
    if (!files[fileName].includes(marker)) {
      throw new Error(`Missing distribution marker in ${fileName}: ${marker}`);
    }
  }
}

console.log("Jolt distribution contract verified");
