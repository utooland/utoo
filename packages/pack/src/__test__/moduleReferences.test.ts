import { afterEach, describe, expect, it, vi } from "vitest";

const config = {
  entry: [],
  module: {
    references: [
      {
        resource: "src/greeting.ts",
        references: [
          {
            id: "greeting",
            factory: { module: "./runtime.js", export: "createReference" },
            data: { operation: "greet" },
          },
        ],
        exports: [{ name: "Greeting", reference: "greeting" }],
      },
    ],
  },
};

afterEach(() => {
  vi.resetModules();
  vi.doUnmock("../binding");
});

describe("native module reference capability", () => {
  it("rejects an older loaded native binary instead of silently keeping the source", async () => {
    vi.doMock("../binding", () => ({}));
    const { serializeConfig } = await import("../core/project");
    const { moduleReferencesVersion } = await import(
      "../core/moduleReferences"
    );
    expect(moduleReferencesVersion).toBe(0);
    await expect(serializeConfig(config, false)).rejects.toThrow(
      "native binary does not support module.references",
    );
    await expect(serializeConfig({ entry: [] }, false)).resolves.toBeTypeOf(
      "string",
    );
  });

  it("forwards the complete reference configuration to a supporting binary", async () => {
    vi.doMock("../binding", () => ({ getModuleReferencesVersion: () => 1 }));
    const { serializeConfig } = await import("../core/project");
    const { moduleReferencesVersion } = await import(
      "../core/moduleReferences"
    );
    expect(moduleReferencesVersion).toBe(1);
    expect(JSON.parse(await serializeConfig(config, false)).module).toEqual(
      config.module,
    );
  });
});
