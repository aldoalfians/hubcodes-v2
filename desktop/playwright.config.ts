import { defineConfig } from "@playwright/test";

// Smoke tests against the mock backend (plain browser, no Tauri, no tmux).
// Uses the installed Chrome, so no browser download is needed.
const port = 5181;

export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  fullyParallel: true,
  reporter: [["list"]],
  use: { baseURL: `http://127.0.0.1:${port}/`, channel: "chrome", viewport: { width: 1440, height: 900 } },
  webServer: {
    command: `pnpm exec vite --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}/`,
    reuseExistingServer: !process.env.CI,
  },
});
