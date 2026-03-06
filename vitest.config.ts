import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    projects: ["packages/pack", "packages/pack-shared"],
    environment: "node",
  },
});
