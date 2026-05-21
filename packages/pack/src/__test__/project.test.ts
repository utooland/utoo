import { describe, expect, it } from "vitest";
import type { ConfigComplete } from "../config/types";
import { serializeConfig } from "../core/project";

const baseConfig: ConfigComplete = {
  entry: [],
};

describe("serializeConfig", () => {
  it("adds default package imports", async () => {
    const serialized = JSON.parse(await serializeConfig(baseConfig, false));

    expect(serialized.optimization.packageImports).toContain("lodash-es");
    expect(serialized.optimization.packageImports).toContain("antd");
    expect(serialized.optimization.packageImports).toContain("react-icons/fa");
    expect(serialized.optimization.packageImports).toContain(
      "@effect/platform-node",
    );
  });

  it("can disable default package imports for isolated comparisons", async () => {
    const previous = process.env.UTOO_DISABLE_DEFAULT_PACKAGE_IMPORTS;
    process.env.UTOO_DISABLE_DEFAULT_PACKAGE_IMPORTS = "1";

    try {
      const serialized = JSON.parse(await serializeConfig(baseConfig, false));

      expect(serialized.optimization.packageImports).toEqual([]);
    } finally {
      if (previous === undefined) {
        delete process.env.UTOO_DISABLE_DEFAULT_PACKAGE_IMPORTS;
      } else {
        process.env.UTOO_DISABLE_DEFAULT_PACKAGE_IMPORTS = previous;
      }
    }
  });

  it("preserves user package imports before defaults without duplicates", async () => {
    const serialized = JSON.parse(
      await serializeConfig(
        {
          ...baseConfig,
          optimization: {
            packageImports: ["custom-package", "lodash-es", "custom-package"],
          },
        },
        false,
      ),
    );

    expect(serialized.optimization.packageImports.slice(0, 2)).toEqual([
      "custom-package",
      "lodash-es",
    ]);
    expect(
      serialized.optimization.packageImports.filter(
        (pkg: string) => pkg === "lodash-es",
      ),
    ).toHaveLength(1);
  });
});
