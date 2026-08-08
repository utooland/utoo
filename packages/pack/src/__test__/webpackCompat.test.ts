import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "fs";
import { tmpdir } from "os";
import { join } from "path";
import { afterEach, describe, expect, it } from "vitest";
import { resolveBundleOptions } from "../config/webpackCompat";

const tempDirs: string[] = [];

afterEach(() => {
  for (const dir of tempDirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

describe("resolveBundleOptions functional externals", () => {
  it("materializes functional externals from discovered source requests", () => {
    const projectDir = mkdtempSync(join(tmpdir(), "utoo-pack-externals-"));
    tempDirs.push(projectDir);
    mkdirSync(join(projectDir, "src"));
    writeFileSync(
      join(projectDir, "package.json"),
      JSON.stringify({ dependencies: { react: "^19.0.0" } }),
    );
    writeFileSync(
      join(projectDir, "src/index.ts"),
      [
        'import React from "react";',
        'import Button from "antd/es/button";',
        'const lodash = require("lodash");',
      ].join("\n"),
    );

    const result = resolveBundleOptions(
      {
        config: {
          entry: [{ import: "./src/index.ts", name: "main" }],
          externals({ request }, callback) {
            if (request === "react" || request === "antd/es/button") {
              callback(null, request, "commonjs");
              return;
            }
            callback();
          },
        },
      },
      projectDir,
    );

    expect(result.config.externals).toEqual({
      react: "commonjs react",
      "antd/es/button": "commonjs antd/es/button",
    });
  });

  it("preserves context for duplicate relative requests", () => {
    const projectDir = mkdtempSync(join(tmpdir(), "utoo-pack-externals-"));
    tempDirs.push(projectDir);
    mkdirSync(join(projectDir, "src/feature"), { recursive: true });
    writeFileSync(
      join(projectDir, "src/index.ts"),
      ['import "./Icon";', 'import "./feature/page";'].join("\n"),
    );
    writeFileSync(join(projectDir, "src/Icon.ts"), "export default 'root';");
    writeFileSync(join(projectDir, "src/feature/page.ts"), 'import "./Icon";');
    writeFileSync(
      join(projectDir, "src/feature/Icon.ts"),
      "export default 'feature';",
    );

    const result = resolveBundleOptions(
      {
        config: {
          entry: [{ import: "./src/index.ts", name: "main" }],
          externals({ context, request }, callback) {
            if (
              request === "./Icon" &&
              context?.endsWith(join("src", "feature"))
            ) {
              callback(null, "FeatureIcon", "commonjs");
              return;
            }
            callback();
          },
        },
      },
      projectDir,
    );

    expect(result.config.externals).toEqual({
      "./Icon": "commonjs FeatureIcon",
    });
  });

  it("ignores malformed webpack entry object values", () => {
    const projectDir = mkdtempSync(join(tmpdir(), "utoo-pack-externals-"));
    tempDirs.push(projectDir);
    mkdirSync(join(projectDir, "src"));
    writeFileSync(
      join(projectDir, "webpack.config.js"),
      "module.exports = {};",
    );
    writeFileSync(
      join(projectDir, "src/index.ts"),
      'import React from "react";',
    );

    const result = resolveBundleOptions(
      {
        webpackMode: true,
        entry: {
          main: { import: "./src/index.ts" },
          missing: {},
          nullish: null,
        } as any,
        externals({ request }, callback) {
          if (request === "react") {
            callback(null, request, "commonjs");
            return;
          }
          callback();
        },
      },
      projectDir,
    );

    expect(result.config.externals).toEqual({
      react: "commonjs react",
    });
  });
});
