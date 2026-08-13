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

  it("preserves config order in global HTML regardless of write order", async () => {
    const outputDir = createTempDir();
    const { appA, appB, manager } = createFixture(outputDir);

    manager.setEntrypoints({
      apps: [appA, appB],
    });
    manager.setWrittenEndpointPath(appB, writtenEndpoint("b.js"));
    manager.setWrittenEndpointPath(appA, writtenEndpoint("a.js"));
    await manager.generateAll();

    const globalHtml = readHtml(outputDir, "all.html");
    expect(globalHtml.indexOf('src="a.js"')).toBeLessThan(
      globalHtml.indexOf('src="b.js"'),
    );
  });

  it("rejects endpoint/config cardinality mismatches", () => {
    const outputDir = createTempDir();
    const manager = new HtmlGenerationManager(
      {
        entry: [
          { name: "first", import: "first.js" },
          { name: "second", import: "second.js" },
        ],
        output: { path: outputDir },
      } as ConfigComplete,
      outputDir,
    );

    expect(() => manager.setEntrypoints({ apps: [{} as Endpoint] })).toThrow(
      "Expected 2 app endpoint(s), received 1",
    );
  });

  it("combines endpoints that generate the same HTML file", async () => {
    const outputDir = createTempDir();
    const firstScript = {} as Endpoint;
    const secondScript = {} as Endpoint;
    const sharedHtml = {
      filename: "page.html",
      templateContent: "<html><head></head><body></body></html>",
    };
    const config = {
      entry: [
        { name: "first", import: "first.js", html: sharedHtml },
        { name: "second", import: "second.js", html: sharedHtml },
      ],
      output: { path: outputDir },
    } as ConfigComplete;
    const manager = new HtmlGenerationManager(config, outputDir);

    manager.setEntrypoints({
      apps: [firstScript, secondScript],
      appPaths: [writtenEndpoint("first.js"), writtenEndpoint("second.js")],
    });
    await manager.generateAll();

    const html = readHtml(outputDir, "page.html");
    expect(html).toContain('src="first.js"');
    expect(html).toContain('src="second.js"');
  });

  it("serializes concurrent writes to global HTML with the latest paths", async () => {
    const outputDir = createTempDir();
    const { appA, appB, manager } = createFixture(outputDir);
    await manager.generateAll();

    manager.setWrittenEndpointPath(appA, writtenEndpoint("a-updated.js"));
    const firstWrite = manager.generateForEndpoint(appA);
    manager.setWrittenEndpointPath(appB, writtenEndpoint("b-updated.js"));
    const secondWrite = manager.generateForEndpoint(appB);
    await Promise.all([firstWrite, secondWrite]);

    const globalHtml = readHtml(outputDir, "all.html");
    expect(globalHtml).toContain('src="a-updated.js"');
    expect(globalHtml).toContain('src="b-updated.js"');
    expect(globalHtml).not.toContain('src="a.js"');
    expect(globalHtml).not.toContain('src="b.js"');
  });

  it("coalesces concurrent updates for the same HTML file", async () => {
    const outputDir = createTempDir();
    const firstScript = {} as Endpoint;
    const secondScript = {} as Endpoint;
    const sharedHtml = { filename: "page.html" };
    const manager = new HtmlGenerationManager(
      {
        entry: [
          { name: "first", import: "first.js", html: sharedHtml },
          { name: "second", import: "second.js", html: sharedHtml },
        ],
        output: { path: outputDir },
      } as ConfigComplete,
      outputDir,
    );
    manager.setEntrypoints({
      apps: [firstScript, secondScript],
      appPaths: [writtenEndpoint("first.js"), writtenEndpoint("second.js")],
    });

    manager.setWrittenEndpointPath(
      firstScript,
      writtenEndpoint("first-new.js"),
    );
    const firstWrite = manager.generateForEndpoint(firstScript);
    manager.setWrittenEndpointPath(
      secondScript,
      writtenEndpoint("second-new.js"),
    );
    const secondWrite = manager.generateForEndpoint(secondScript);
    await Promise.all([firstWrite, secondWrite]);

    const html = readHtml(outputDir, "page.html");
    expect(html).toContain('src="first-new.js"');
    expect(html).toContain('src="second-new.js"');
  });
});
