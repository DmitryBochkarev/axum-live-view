import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  timeout: 30000,
  retries: 0,
  workers: 1,
  use: {
    headless: true,
    baseURL: "http://localhost:3000",
  },
  webServer: undefined, // We manage the server ourselves
});
