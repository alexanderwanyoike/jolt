import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import packageJson from "./package.json";

export default defineConfig({
  clearScreen: false,
  define: {
    __JOLT_CONSOLE_VERSION__: JSON.stringify(packageJson.version)
  },
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true
  },
  envPrefix: ["VITE_", "TAURI_"],
  test: {
    environment: "jsdom",
    setupFiles: ["src/test/setup.ts"]
  }
});
