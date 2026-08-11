import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it } from "vitest";
import type { NapiWrittenEndpoint } from "../binding";
import type { ConfigComplete, HtmlConfig } from "../config/types";
import type { Endpoint } from "../core/types";
import { HtmlGenerationManager } from "../utils/HtmlGenerationManager";

const tempDirs: string[] = [];

function createTempDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "utoo-html-manager-"));
  tempDirs.push(dir);
  return dir;
}

function writtenEndpoint(...clientPaths: string[]): NapiWrittenEndpoint {
  return {
    type: "nodejs",
    entryPath: "dist",
    clientPaths,
    serverPaths: [],
    config: {},
  };
}

function readHtml(outputDir: string, filename: string) {
  return fs.readFileSync(path.join(outputDir, filename), "utf8");
}

function createFixture(outputDir: string) {
  const appA = {} as Endpoint;
  const appB = {} as Endpoint;
  const library = {} as Endpoint;
  const config: ConfigComplete & { html: HtmlConfig } = {
    entry: [
      {
        name: "a",
        import: "a.js",
        html: { filename: "a.html", title: "A" },
      },
      {
        name: "b",
        import: "b.js",
        html: { filename: "b.html", title: "B" },
      },
      {
        name: "library",
        import: "library.js",
        library: { name: "Library" },
        html: { filename: "library.html", title: "Library" },
      },
    ],
    output: { path: outputDir },
    html: { filename: "all.html", title: "All" },
  };
  const manager = new HtmlGenerationManager(config, outputDir);
  manager.setEntrypoints({
    apps: [appA, appB],
    libraries: [library],
    appPaths: [writtenEndpoint("a.js", "a.css"), writtenEndpoint("b.js")],
    libraryPaths: [writtenEndpoint("library.js")],
  });
  return { appA, appB, library, manager };
}

afterEach(() => {
  for (const dir of tempDirs) {
    fs.rmSync(dir, { force: true, recursive: true });
  }
  tempDirs.length = 0;
});

describe("HtmlGenerationManager", () => {
  it("injects only the owning endpoint assets into entry HTML", async () => {
    const outputDir = createTempDir();
    const { manager } = createFixture(outputDir);

    await manager.generateAll();

    const appAHtml = readHtml(outputDir, "a.html");
    expect(appAHtml).toContain('src="a.js"');
    expect(appAHtml).toContain('href="a.css"');
    expect(appAHtml).not.toContain('src="b.js"');
    expect(appAHtml).not.toContain('src="library.js"');

    const libraryHtml = readHtml(outputDir, "library.html");
    expect(libraryHtml).toContain('src="library.js"');
    expect(libraryHtml).not.toContain('src="a.js"');

    const globalHtml = readHtml(outputDir, "all.html");
    expect(globalHtml).toContain('src="a.js"');
    expect(globalHtml).toContain('src="b.js"');
    expect(globalHtml).toContain('src="library.js"');
  });

  it("regenerates only global and owning entry HTML after an update", async () => {
    const outputDir = createTempDir();
    const { appA, manager } = createFixture(outputDir);
    await manager.generateAll();
    fs.writeFileSync(path.join(outputDir, "b.html"), "unchanged");

    manager.setWrittenEndpointPath(appA, writtenEndpoint("a-updated.js"));
    await manager.generateForEndpoint(appA);

    const appAHtml = readHtml(outputDir, "a.html");
    expect(appAHtml).toContain('src="a-updated.js"');
    expect(appAHtml).not.toContain('src="a.js"');
    expect(readHtml(outputDir, "b.html")).toBe("unchanged");

    const globalHtml = readHtml(outputDir, "all.html");
    expect(globalHtml).toContain('src="a-updated.js"');
    expect(globalHtml).toContain('src="b.js"');
  });
});
