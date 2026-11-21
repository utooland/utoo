const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { test } = require("node:test");
const { setTimeout: delay } = require("node:timers/promises");
const { serve } = require("../cjs/commands/dev.js");

const repository = path.resolve(__dirname, "../../..");
const policy = { timeout: { action: "none" }, afterBuild: "hold" };

async function start(t, options = {}, config = {}) {
  const project = fs.mkdtempSync(
    path.join(repository, "examples/.edit-batch-test-"),
  );
  fs.cpSync(path.join(__dirname, "fixtures/edit-batch"), project, {
    recursive: true,
  });
  const publications = [];
  const oldTitle = process.title;
  const oldNodeEnv = process.env.NODE_ENV;
  let handle;
  t.after(async () => {
    try {
      await handle?.close();
    } finally {
      fs.rmSync(project, { recursive: true, force: true });
      process.title = oldTitle;
      if (oldNodeEnv === undefined) delete process.env.NODE_ENV;
      else process.env.NODE_ENV = oldNodeEnv;
    }
  });
  handle = await serve(
    {
      tracing: false,
      config: {
        entry: [{ name: "main", import: "./index.jsx", html: {} }],
        output: { path: "./dist", clean: true },
        persistentCaching: false,
        react: { runtime: "classic" },
        ...config,
      },
    },
    project,
    repository,
    {
      port: 0,
      hostname: "127.0.0.1",
      returnHandle: true,
      handleSignals: false,
      logServerInfo: false,
      editBatch: policy,
      ...options,
      async onClientBuild(context) {
        publications.push({
          ...context,
          clientPaths: [...context.clientPaths],
        });
        await options.onClientBuild?.(context);
      },
    },
  );
  const value = (text) =>
    fs.writeFileSync(
      path.join(project, "value.js"),
      `export const label = ${JSON.stringify(text)};\n`,
    );
  function publishedText() {
    // The published entry is a runtime bootstrap; business code lives in module chunks.
    // Each asserted marker is new in this commit. The browser case separately checks
    // that the active page references and executes the final module graph.
    function readModules(directory) {
      return fs
        .readdirSync(directory, { withFileTypes: true })
        .flatMap((entry) => {
          const file = path.join(directory, entry.name);
          return entry.isDirectory()
            ? readModules(file)
            : entry.name.endsWith(".js")
              ? [fs.readFileSync(file, "utf8")]
              : [];
        });
    }
    return readModules(path.join(project, "dist")).join("\n");
  }
  return { handle, project, publications, value, publishedText };
}

test(
  "native commit flushes final files without touching the entry and keeps later edits held",
  { timeout: 120_000 },
  async (t) => {
    const f = await start(t);
    const entry = path.join(f.project, "index.jsx");
    const originalEntry = fs.readFileSync(entry, "utf8");
    const originalMtime = fs.statSync(entry).mtimeMs;
    const id = await f.handle.beginEditBatch();
    const before = f.publications.length;
    f.value("batch-half");
    await delay(1100);
    assert.equal(f.publications.length, before);
    f.value("batch-final");
    const result = await f.handle.commitEditBatch(id);
    assert.equal(result.outcome, "built");
    assert.equal(result.watchState, "held");
    assert.match(f.publishedText(), /batch-final/);
    assert.equal(await f.handle.waitForEditBatch(id), result);
    assert.equal(await f.handle.commitEditBatch(id), result);
    assert.equal(fs.readFileSync(entry, "utf8"), originalEntry);
    assert.equal(fs.statSync(entry).mtimeMs, originalMtime);

    const published = f.publications.length;
    f.value("batch-next");
    await delay(150);
    assert.equal(f.publications.length, published);
    assert.equal(
      (await f.handle.commitEditBatch(await f.handle.beginEditBatch())).outcome,
      "built",
    );
    assert.match(f.publishedText(), /batch-next/);
    // No-change commit must also settle, without assuming zero writes or a task-count delta.
    assert.equal(
      (await f.handle.commitEditBatch(await f.handle.beginEditBatch())).outcome,
      "built",
    );
  },
);

test(
  "native close keeps an uncommitted draft held and stops invalidation before later writes",
  { timeout: 120_000 },
  async (t) => {
    const f = await start(t);
    const id = await f.handle.beginEditBatch();
    const publications = f.publications.length;
    f.value("closed-uncommitted-draft");
    await f.handle.close();
    assert.equal(f.publications.length, publications);
    assert.equal((await f.handle.waitForEditBatch(id)).watchState, "closed");
    // Exercise queued notify events after the engine's storage has been dropped.
    f.value("write-after-close");
    fs.rmSync(path.join(f.project, "value.js"));
    await delay(1100);
  },
);

test(
  "native snapshots compare ordered endpoint identities and project ownership",
  { timeout: 120_000 },
  async (t) => {
    const { createHotReloader } = require("../cjs/core/hmr.js");
    const { resolveBundleOptions } = require("../cjs/config/webpackCompat.js");
    const projects = [];
    t.after(async () => {
      for (const f of projects.reverse()) {
        try {
          await f.reloader?.close();
        } finally {
          fs.rmSync(f.directory, { recursive: true, force: true });
        }
      }
    });
    const config = {
      entry: [
        { name: "alpha", import: "./a.js" },
        { name: "beta", import: "./b.js" },
      ],
      output: { path: "./dist" },
      persistentCaching: false,
    };
    for (let i = 0; i < 2; i++) {
      const f = {
        directory: fs.mkdtempSync(
          path.join(repository, "examples/.edit-batch-test-"),
        ),
      };
      projects.push(f);
      fs.writeFileSync(
        path.join(f.directory, "a.js"),
        'export const a = "alpha";\n',
      );
      fs.writeFileSync(
        path.join(f.directory, "b.js"),
        'export const b = "beta";\n',
      );
      f.reloader = await createHotReloader(
        resolveBundleOptions(
          { tracing: false, config: structuredClone(config) },
          f.directory,
          repository,
        ),
        f.directory,
        repository,
      );
      f.project = f.reloader.turbopackProject;
      await f.project.setWatchBatchHold(true);
    }
    const first = await projects[0].project.readEntrypoints();
    assert.equal(
      first.entrypoints.apps.length,
      2,
      "fixture must contain two real app endpoints",
    );
    assert(first.sameAs(await projects[0].project.readEntrypoints()));
    const other = await projects[1].project.readEntrypoints();
    assert.equal(first.sameAs(other), false);
    assert.equal(other.sameAs(first), false);
    // Different factories have different JS token maps: call native comparison too.
    const binding = require("../cjs/binding.js");
    const left = await binding.projectReadEntrypoints(
      projects[0].project._nativeProject,
    );
    const right = await binding.projectReadEntrypoints(
      projects[1].project._nativeProject,
    );
    assert.equal(
      binding.entrypointsSnapshotsEqual(left.snapshot, right.snapshot),
      false,
    );
    assert.equal(
      binding.entrypointsSnapshotsEqual(right.snapshot, left.snapshot),
      false,
    );
    await projects[0].project.update({
      dev: true,
      config: {
        ...config,
        mode: "development",
        entry: [...config.entry].reverse(),
      },
    });
    const reordered = await projects[0].project.readEntrypoints();
    assert.equal(
      first.entrypoints.apps.length,
      reordered.entrypoints.apps.length,
    );
    assert.equal(
      first.sameAs(reordered),
      false,
      "same-length entry reorder must change the native snapshot",
    );
    assert.equal(
      reordered.sameAs(first),
      false,
      "reorder comparison must be symmetric",
    );
    assert(reordered.sameAs(await projects[0].project.readEntrypoints()));
  },
);

test(
  "native resume returns to ordinary realtime updates after a committed batch",
  { timeout: 120_000 },
  async (t) => {
    const f = await start(t, {
      editBatch: { ...policy, afterBuild: "resume" },
    });
    const id = await f.handle.beginEditBatch();
    f.value("resume-committed");
    assert.deepEqual(await f.handle.commitEditBatch(id), {
      batchId: id,
      outcome: "built",
      reason: "commit",
      watchState: "watching",
    });
    assert.match(f.publishedText(), /resume-committed/);
    f.value("resume-realtime");
    const deadline = Date.now() + 15_000;
    while (!f.publishedText().includes("resume-realtime")) {
      assert(
        Date.now() < deadline,
        "ordinary saves must publish after resume ACK",
      );
      await delay(50);
    }
  },
);

test(
  "native syntax diagnostics settle a failed batch and a new commit repairs it",
  { timeout: 120_000 },
  async (t) => {
    const f = await start(t);
    const id = await f.handle.beginEditBatch();
    fs.writeFileSync(
      path.join(f.project, "value.js"),
      "export const label = ;\n",
    );
    const failed = await f.handle.commitEditBatch(id);
    assert.equal(failed.outcome, "failed");
    assert.equal(failed.watchState, "held");
    assert(
      failed.errors.some(
        (error) =>
          error.code === "COMPILATION_FAILED" &&
          error.issues.some(
            (issue) =>
              issue.filePath.includes("value.js") &&
              (issue.description || issue.title),
          ),
      ),
    );
    f.value("batch-repaired");
    const repaired = await f.handle.commitEditBatch(
      await f.handle.beginEditBatch(),
    );
    assert.equal(repaired.outcome, "built");
    assert.match(f.publishedText(), /batch-repaired/);
    assert.equal(await f.handle.waitForEditBatch(id), failed);
  },
);

test(
  "explicit edit batch configuration rejects lazy activation before serving",
  { timeout: 120_000 },
  async (t) => {
    await assert.rejects(
      start(t, {}, { devServer: { lazyDynamicImports: true } }),
      { code: "UNSUPPORTED_BATCH_CONFIGURATION" },
    );
  },
);

test(
  "realtime lazy activation keeps serving when a batch request is rejected",
  { timeout: 120_000 },
  async (t) => {
    const f = await start(
      t,
      { editBatch: undefined },
      { devServer: { lazyDynamicImports: true } },
    );
    await assert.rejects(f.handle.beginEditBatch(), {
      code: "UNSUPPORTED_BATCH_CONFIGURATION",
    });
    const response = await fetch(
      `http://127.0.0.1:${f.handle.port}/index.html`,
    );
    assert.equal(response.status, 200);
    assert.match(await response.text(), /main/);
  },
);

test(
  "Node-produced HMR hides cross-second intermediate edits and preserves React state",
  { timeout: 120_000 },
  async (t) => {
    const { chromium } = require("@playwright/test");
    const browser = await chromium.launch({
      headless: true,
      args: ["--no-sandbox"],
    });
    t.after(() => browser.close());
    const f = await start(t);
    const page = await browser.newPage();
    const runtimeErrors = [];
    page.on("pageerror", (error) => runtimeErrors.push(error.message));
    await page.goto(`http://127.0.0.1:${f.handle.port}/index.html`);
    await page.waitForFunction(
      () => document.querySelector("#label")?.textContent === "batch-initial",
    );
    await page.click("#increment");
    await page.waitForFunction(
      () => document.querySelector("#increment")?.textContent === "Count: 1",
    );
    const id = await f.handle.beginEditBatch();
    f.value("batch-half");
    fs.writeFileSync(
      path.join(f.project, "style.css"),
      "#label { color: rgb(200, 0, 0); }\n",
    );
    await delay(1100);
    assert.equal(await page.textContent("#label"), "batch-initial");
    assert.equal(
      await page.$eval("#label", (element) => getComputedStyle(element).color),
      "rgb(10, 20, 30)",
    );
    f.value("batch-final");
    fs.writeFileSync(
      path.join(f.project, "style.css"),
      "#label { color: rgb(0, 128, 0); }\n",
    );
    assert.equal((await f.handle.commitEditBatch(id)).outcome, "built");
    await page.waitForFunction(
      () => document.querySelector("#label")?.textContent === "batch-final",
    );
    await page.waitForFunction(
      () =>
        getComputedStyle(document.querySelector("#label")).color ===
        "rgb(0, 128, 0)",
    );
    assert.equal(await page.textContent("#increment"), "Count: 1");
    assert.deepEqual(runtimeErrors, []);
    if (process.env.UTOO_PACK_TEST_SCREENSHOT)
      await page.screenshot({
        path: process.env.UTOO_PACK_TEST_SCREENSHOT,
        fullPage: true,
      });
  },
);
