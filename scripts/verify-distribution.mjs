import { readFileSync } from "node:fs";

const files = {
  workflow: readFileSync(".github/workflows/package-jolt-console.yml", "utf8"),
  tauriConfig: readFileSync("apps/jolt-console/src-tauri/tauri.conf.json", "utf8"),
  installer: readFileSync("scripts/install-jolt-console.sh", "utf8"),
  packageScript: readFileSync("scripts/package-jolt-console.sh", "utf8"),
  normalizeArtifacts: readFileSync("scripts/normalize-jolt-console-artifacts.sh", "utf8"),
  assembleRelease: readFileSync("scripts/assemble-jolt-console-release.sh", "utf8"),
  updateManifest: readFileSync("scripts/write-jolt-console-update-manifest.mjs", "utf8"),
  readme: readFileSync("README.md", "utf8"),
  distributionCard: readFileSync("docs/cards/077-jolt-distribution-v0.md", "utf8"),
  installCliCard: readFileSync("docs/cards/084-install-jolt-cli-with-console.md", "utf8")
};

const requiredMarkers = {
  workflow: [
    "Package Jolt Console",
    "matrix:",
    "ubuntu-22.04",
    "macos-latest",
    "windows-latest",
    "scripts/package-jolt-console.sh",
    "scripts/normalize-jolt-console-artifacts.sh",
    "scripts/assemble-jolt-console-release.sh",
    "shell: bash",
    "jolt-console-x86_64.AppImage",
    "jolt-console-aarch64.dmg",
    "jolt-console-aarch64.app.tar.gz",
    "jolt-console-x86_64-setup.exe",
    "actions/upload-artifact",
    "softprops/action-gh-release",
    "refs/tags/",
    "jolt-linux-x86_64",
    "jolt-macos-aarch64",
    "jolt-windows-x86_64.exe",
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
  tauriConfig: ["icons/icon.png", "icons/icon.ico"],
  packageScript: [
    "target/release/bundle/appimage",
    "target/release/bundle/dmg",
    "target/release/bundle/macos",
    "target/release/bundle/nsis",
    "TAURI_BUILD_ARGS",
    "BUNDLE_KIND",
    "--bundle",
    "Prefetching Tauri AppImage helper binaries",
    "linuxdeploy-x86_64.AppImage",
    "JOLT_CREATE_UPDATER_ARTIFACTS",
    "createUpdaterArtifacts"
  ],
  normalizeArtifacts: [
    "Normalize Jolt Console package artifacts",
    "--bundle",
    "appimage",
    "dmg",
    "nsis",
    "target/release/bundle/appimage",
    "target/release/bundle/dmg",
    "target/release/bundle/macos",
    "target/release/bundle/nsis",
    "sha256sum",
    "shasum -a 256"
  ],
  assembleRelease: [
    "Assemble normalized Jolt Console artifacts",
    "jolt-console-x86_64.AppImage.sig",
    "jolt-console-aarch64.app.tar.gz.sig",
    "jolt-console-x86_64-setup.exe.sig",
    "jolt-linux-x86_64.sha256",
    "jolt-macos-aarch64.sha256",
    "jolt-windows-x86_64.exe.sha256",
    "write-jolt-console-update-manifest.mjs",
    "latest.json",
    "linux-x86_64",
    "darwin-aarch64",
    "windows-x86_64"
  ],
  updateManifest: [
    "latest.json",
    "linux-x86_64",
    "darwin-aarch64",
    "windows-x86_64",
    "signature",
    "jolt-console-x86_64.AppImage",
    "jolt-console-aarch64.app.tar.gz",
    "jolt-console-x86_64-setup.exe"
  ],
  readme: [
    "curl -fsSL",
    "scripts/install-jolt-console.sh",
    "JOLT_VERSION=",
    "jolt-console --appimage-help",
    "jolt-console-aarch64.dmg",
    "jolt-console-aarch64.app.tar.gz",
    "jolt-console-x86_64-setup.exe",
    "jolt --version",
    "--cli-only"
  ],
  distributionCard: [
    "GitHub Actions",
    "jolt-console-x86_64.AppImage",
    "jolt-console-aarch64.dmg",
    "jolt-console-x86_64-setup.exe",
    "install-jolt-console.sh"
  ],
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
