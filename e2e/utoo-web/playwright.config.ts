import { defineConfig } from "@playwright/test";

const baseURL = process.env.UTOOWEB_DEMO_URL ?? "http://127.0.0.1:8081";

export default defineConfig({
  testDir: ".",
  timeout: 15 * 60 * 1000,
  expect: {
    timeout: 60 * 1000,
  },
  fullyParallel: false,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI
    ? [["github"], ["html", { open: "never" }]]
    : [["list"], ["html", { open: "never" }]],
  use: {
    baseURL,
    browserName: "chromium",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    viewport: { width: 1600, height: 1000 },
  },
});
