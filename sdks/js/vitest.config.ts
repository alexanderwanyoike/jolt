import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

// The guide's sample app (guide/src) imports the SDK by its published name,
// exactly as a real app would; resolve those imports to the local sources so
// the samples are tested against the code in this tree.
const src = (file: string) => fileURLToPath(new URL(`./src/${file}`, import.meta.url));

export default defineConfig({
  resolve: {
    alias: {
      "jolt-sdk/transport-http": src("transport-http.ts"),
      "jolt-sdk/transport-tauri": src("transport-tauri.ts"),
      "jolt-sdk/testing": src("testing.ts"),
      "jolt-sdk/data": src("data.ts"),
      "jolt-sdk": src("index.ts"),
    },
  },
});
