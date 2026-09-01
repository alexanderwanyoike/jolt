import { readFileSync } from "node:fs";

const files = {
  workspaceCargo: readFileSync("Cargo.toml", "utf8"),
  workflow: readFileSync(".github/workflows/package-jolt-console.yml", "utf8"),
  tauriConfig: readFileSync("apps/jolt-console/src-tauri/tauri.conf.json", "utf8"),
  installer: readFileSync("scripts/install-jolt-console.sh", "utf8"),
  packageScript: readFileSync("scripts/package-jolt-console.sh", "utf8"),
  normalizeArtifacts: readFileSync("scripts/normalize-jolt-console-artifacts.sh", "utf8"),
  assembleRelease: readFileSync("scripts/assemble-jolt-console-release.sh", "utf8"),
  updateManifest: readFileSync("scripts/write-jolt-console-update-manifest.mjs", "utf8"),
  readme: readFileSync("README.md", "utf8")
};

function tomlSection(source, name) {
  const header = `[${name}]`;
  const start = source.indexOf(header);
  if (start === -1) return null;

  const body = source.slice(start + header.length);
  const nextSection = body.search(/^\[/m);
  return nextSection === -1 ? body : body.slice(0, nextSection);
}

const releaseProfile = tomlSection(files.workspaceCargo, "profile.release");
if (releaseProfile === null || !/^strip\s*=\s*true\s*$/m.test(releaseProfile)) {
  throw new Error("Release binaries must be stripped before packaging");
}

if (
  !files.packageScript.includes("node_modules/.bin/tauri") ||
  files.packageScript.includes("npm run tauri")
) {
  throw new Error(
    "Release packaging must invoke the local Tauri CLI without restaging the debug daemon"
  );
}

if (
  !files.packageScript.includes(
    'TAURI_BUILD_ARGS=(build --bundles "$TAURI_BUNDLE_KIND")'
  )
) {
  throw new Error(
    "Direct Tauri packaging arguments must not forward --bundles to Cargo"
  );
}

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
    "jolt-macos-aarch64",
    "jolt-windows-x86_64.exe",
    "JOLT_INSTALL_OS",
    "JOLT_INSTALL_ARCH",
    "JOLT_CLI_ASSET_NAME",
    "JOLT_CLI_BIN_NAME",
    "jolt.exe",
    "Console direct install is only supported for the Linux AppImage.",
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
    "app,dmg",
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
    "JOLT_REQUIRE_UPDATER_ARTIFACTS",
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
    "jolt-macos-aarch64",
    "jolt-windows-x86_64.exe",
    "jolt --version",
    "--cli-only"
  ]
};

for (const [fileName, markers] of Object.entries(requiredMarkers)) {
  for (const marker of markers) {
    if (!files[fileName].includes(marker)) {
      throw new Error(`Missing distribution marker in ${fileName}: ${marker}`);
    }
  }
}

console.log("Jolt distribution contract verified");
