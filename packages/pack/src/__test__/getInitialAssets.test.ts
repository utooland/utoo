import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it } from "vitest";
import {
  getInitialAssetsFromEndpointPaths,
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

  it("uses endpoint client paths for html assets without stats", () => {
    expect(
      getInitialAssetsFromEndpointPaths([
        {
          type: "nodejs",
          entryPath: "dist",
          clientPaths: [
            "static/runtime.js",
            "static/runtime.js",
            "static/runtime.js.map",
            "static/runtime.js.LICENSE.txt",
            "static/app.css",
          ],
          serverPaths: [],
          config: {},
        },
      ]),
    ).toEqual({
      js: ["static/runtime.js"],
      css: ["static/app.css"],
    });
  });
});
