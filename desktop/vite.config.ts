import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import pkg from "./package.json" with { type: "json" };

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  // Relative asset URLs: the same build works at / (Tauri) and at /canvas/ (hubcodes serve).
  base: "./",
  plugins: [react()],
  // The app version, shown in Settings; scripts/bump-version.sh keeps package.json in step.
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: { target: "safari16", sourcemap: false },
});
