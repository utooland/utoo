import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it } from "vitest";
import {
  getInitialAssetsFromOutput,
  getInitialAssetsFromStats,
} from "../utils/getInitialAssets";

let tempDirs: string[] = [];

function createTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "utoo-assets-"));
  tempDirs.push(dir);
  return dir;
}

afterEach(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { force: true, recursive: true });
  }
  tempDirs = [];
});

describe("initial asset discovery", () => {
  it("deduplicates entry assets from webpack stats", () => {
    const outputDir = createTempDir();
    fs.writeFileSync(
      path.join(outputDir, "stats.json"),
      JSON.stringify({
        entrypoints: {
          main: {
            assets: [
              { name: "index.js" },
              { name: "index.js" },
              { name: "index.css" },
            ],
          },
        },
      }),
    );

    expect(getInitialAssetsFromStats(outputDir)).toEqual({
      js: ["index.js"],
      css: ["index.css"],
    });
  });

  it("finds entry javascript and css from output files without stats", () => {
    const outputDir = createTempDir();
    for (const file of [
      "index.js",
      "index.js.map",
      "chunk_123.js",
      "index_456.css",
      "stats.json",
    ]) {
      fs.writeFileSync(path.join(outputDir, file), "");
    }

    expect(getInitialAssetsFromOutput(outputDir, ["./src/index.jsx"])).toEqual({
      js: ["index.js"],
      css: ["index_456.css"],
    });
  });

  it("finds hashed entry javascript when output includes multiple chunks", () => {
    const outputDir = createTempDir();
    for (const file of [
      "index.a8f3b2c1.js",
      "chunk-vendors.12345678.js",
      "lazy-route.87654321.js",
      "index.a8f3b2c1.js.map",
      "index.a8f3b2c1.js.LICENSE.txt",
      "index.a8f3b2c1.css",
    ]) {
      fs.writeFileSync(path.join(outputDir, file), "");
    }

    expect(getInitialAssetsFromOutput(outputDir, ["./src/index.jsx"])).toEqual({
      js: ["index.a8f3b2c1.js"],
      css: ["index.a8f3b2c1.css"],
    });
  });
});
