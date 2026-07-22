import { expect, test, type Page } from "@playwright/test";
import fs from "node:fs";
import path from "node:path";

const sharedPath = path.join(__dirname, "src/shared.js");
const sharedCssPath = path.join(__dirname, "src/shared.css");
const indexPath = path.join(__dirname, "src/index.js");
const baseline = fs.readFileSync(sharedPath, "utf8");
const cssBaseline = fs.readFileSync(sharedCssPath, "utf8");
const indexBaseline = fs.readFileSync(indexPath, "utf8");
const pageFrames = new WeakMap<Page, string[]>();
const pageReadySockets = new WeakMap<Page, Set<object>>();
let restoreGeneration = 0;

test.beforeEach(async ({ page }) => {
  const frames: string[] = [];
  const readySockets = new Set<object>();
  pageFrames.set(page, frames);
  pageReadySockets.set(page, readySockets);
  page.on("websocket", (socket) => {
    socket.on("framereceived", ({ payload }) => {
      const frame = String(payload);
      frames.push(frame);
      if (frameAction(frame) === "turbopack-connected") {
        readySockets.add(socket);
      }
    });
    socket.on("close", () => readySockets.delete(socket));
  });
});

function writeShared(version: string) {
  fs.writeFileSync(sharedPath, baseline.replace("shared-v1", version));
}

function writeSharedCss(version: string) {
  fs.writeFileSync(
    sharedCssPath,
    cssBaseline.replace("shared-css-v1", version),
  );
}

async function readRuntimeState(page: Page) {
  return page.evaluate(() => ({
    evaluations: (globalThis as any).__packHmrSharedEvaluations,
    entryEvaluations: (globalThis as any).__packHmrEntryEvaluations,
    hmrApiAvailable: (globalThis as any).__packHmrApiAvailable,
    pageToken: (globalThis as any).__packHmrPageToken,
    timeOrigin: performance.timeOrigin,
  }));
}

function frameAction(frame: string) {
  try {
    return JSON.parse(frame).action as string | undefined;
  } catch {
    return undefined;
  }
}

async function mutateAndWaitForBuild(page: Page, mutate: () => void) {
  const frames = pageFrames.get(page) ?? [];
  await expect
    .poll(() => pageReadySockets.get(page)?.size ?? 0)
    .toBeGreaterThan(0);
  const frameStart = frames.length;
  let buildInProgress = false;
  for (const frame of frames.slice(0, frameStart)) {
    const action = frameAction(frame);
    if (action === "building") buildInProgress = true;
    if (action === "built") buildInProgress = false;
  }
  mutate();
  try {
    await expect
      .poll(() => {
        // A mutation can join compilation work that was already started by a
        // lazy chunk load. In that case BUILDING legitimately precedes
        // frameStart. Preserve that unmatched BUILDING state, but do not treat
        // a subscription baseline message as mutation-triggered activity.
        let activity = buildInProgress;
        for (const frame of frames.slice(frameStart)) {
          const action = frameAction(frame);
          if (action === "building") activity = true;
          if (activity && action === "built") return true;
        }
        return false;
      })
      .toBe(true);
  } catch (error) {
    console.error(
      "HMR frames received after the test mutation:",
      frames.slice(frameStart).map(frameAction),
    );
    throw error;
  }
}

test.afterEach(async () => {
  const marker = `// e2e-restore-${++restoreGeneration}`;
  fs.writeFileSync(sharedPath, baseline);
  fs.writeFileSync(sharedCssPath, cssBaseline);
  fs.writeFileSync(indexPath, `${indexBaseline}\n${marker}\n`);

  // A race test may deliberately reload or disconnect the page, so raw socket
  // frames are not a reliable cleanup signal. The emitted entry itself proves
  // the watcher rebuilt the complete baseline graph.
  await expect
    .poll(() => {
      const outputDir = path.join(__dirname, "dist");
      if (!fs.existsSync(outputDir)) return false;
      return fs
        .readdirSync(outputDir)
        .filter((file) => file.endsWith(".js"))
        .some((file) =>
          fs.readFileSync(path.join(outputDir, file), "utf8").includes(marker),
        );
    })
    .toBe(true);
});

test.afterAll(() => {
  fs.writeFileSync(sharedPath, baseline);
  fs.writeFileSync(sharedCssPath, cssBaseline);
  fs.writeFileSync(indexPath, indexBaseline);
});

function getTurbopackUpdates(frames: string[]) {
  return getTurbopackEnvelopes(frames).flat();
}

function getTurbopackEnvelopes(frames: string[]) {
  return frames.flatMap((frame) => {
    try {
      const message = JSON.parse(frame);
      if (message.action !== "turbopack-message") return [];
      return [Array.isArray(message.data) ? message.data : [message.data]];
    } catch {
      return [];
    }
  });
}

test("closes the first dynamic-load subscription race", async ({ page }) => {
  await page.addInitScript(() => {
    const originalSend = WebSocket.prototype.send;
    const delayedFrames: Array<{ socket: WebSocket; data: any }> = [];
    (globalThis as any).__delayedDynamicSubscriptions = [];
    (globalThis as any).__hmrSubscriptionPaths = [];
    (globalThis as any).__hmrSubscriptionVersions = [];
    (globalThis as any).__flushDynamicSubscriptions = () => {
      sessionStorage.setItem("pack-hmr-disable-subscription-delay", "1");
      for (const { socket, data } of delayedFrames.splice(0)) {
        originalSend.call(socket, data);
      }
    };
    WebSocket.prototype.send = function (data) {
      let message;
      try {
        message = JSON.parse(String(data));
      } catch {}
      if (message?.type === "turbopack-subscribe") {
        (globalThis as any).__hmrSubscriptionPaths.push(message.path);
        (globalThis as any).__hmrSubscriptionVersions.push(message.version);
      }
      if (
        sessionStorage.getItem("pack-hmr-disable-subscription-delay") !== "1" &&
        message?.type === "turbopack-subscribe" &&
        /(?:^|\/)src_[ab]_[^/]+\.js$/.test(message.path)
      ) {
        const paths = (globalThis as any).__delayedDynamicSubscriptions;
        if (!paths.includes(message.path)) paths.push(message.path);
        delayedFrames.push({ socket: this, data });
        return;
      }
      originalSend.call(this, data);
    };
  });

  await page.goto("/");
  const initial = await readRuntimeState(page);
  await page.locator("#load").click();
  await expect
    .poll(() =>
      page.evaluate(
        () => (globalThis as any).__delayedDynamicSubscriptions.length,
      ),
    )
    .toBe(2);

  await expect
    .poll(() =>
      page.evaluate(() =>
        (globalThis as any).__hmrSubscriptionVersions.every(
          (version: unknown) => typeof version === "string",
        ),
      ),
    )
    .toBe(true);
  await mutateAndWaitForBuild(page, () => writeShared("shared-race-v2"));
  await page.evaluate(() => (globalThis as any).__flushDynamicSubscriptions());

  // The server baseline is newer than the manifest that initiated the load.
  // Reject that manifest and reload instead of evaluating stale factories.
  await expect
    .poll(() => readRuntimeState(page).then((state) => state.pageToken))
    .not.toBe(initial.pageToken);
  await page.locator("#load").click();
  await expect(page.locator("#a")).toHaveText("a:shared-race-v2");
  await expect(page.locator("#b")).toHaveText("b:shared-race-v2");
  await expect(page.locator("#error")).toBeEmpty();
  const updated = await readRuntimeState(page);
  const subscriptionPaths = await page.evaluate(
    () => (globalThis as any).__hmrSubscriptionPaths,
  );
  const subscriptionCounts = new Map<string, number>();
  for (const path of subscriptionPaths) {
    subscriptionCounts.set(path, (subscriptionCounts.get(path) ?? 0) + 1);
  }
  expect([...subscriptionCounts.values()].sort()).toEqual([1, 2, 2]);
  expect(updated.evaluations).toBe(1);
  expect(updated.hmrApiAvailable).toBe(true);
  expect(updated.pageToken).not.toBe(initial.pageToken);
  expect(updated.timeOrigin).not.toBe(initial.timeOrigin);
});

test("revalidates after HTTP members load before exposing a dynamic import", async ({
  page,
}) => {
  let releaseMembers!: () => void;
  const membersReleased = new Promise<void>((resolve) => {
    releaseMembers = resolve;
  });
  let capturedMembers = 0;

  await page.route(/\.js\?hmr=/, async (route) => {
    // Hold the V1 URL until after the build, then fetch it from the live output
    // directory. The response is V2 even though the URL contains V1's token.
    // Post-load subscription validation must detect that race.
    capturedMembers += 1;
    await membersReleased;
    const response = await route.fetch();
    const body = await response.body();
    await route.fulfill({ response, body });
  });

  try {
    await page.goto("/");
    const initial = await readRuntimeState(page);
    await page.locator("#load").click();
    await expect.poll(() => capturedMembers).toBeGreaterThanOrEqual(2);

    await mutateAndWaitForBuild(page, () => writeShared("shared-queued-v2"));
    releaseMembers();

    await expect
      .poll(() => readRuntimeState(page).then((state) => state.pageToken))
      .not.toBe(initial.pageToken);
    await page.locator("#load").click();
    await expect(page.locator("#a")).toHaveText("a:shared-queued-v2");
    await expect(page.locator("#b")).toHaveText("b:shared-queued-v2");
    await expect(page.locator("#error")).toBeEmpty();
    const updated = await readRuntimeState(page);
    expect(updated.evaluations).toBe(1);
    expect(updated.pageToken).not.toBe(initial.pageToken);
    expect(updated.timeOrigin).not.toBe(initial.timeOrigin);
  } finally {
    releaseMembers();
  }
});

test("resubscribes dynamic imports after the HMR socket reconnects", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const NativeWebSocket = WebSocket;
    const sockets: WebSocket[] = [];
    (globalThis as any).__packHmrSockets = sockets;
    (globalThis as any).WebSocket = new Proxy(NativeWebSocket, {
      construct(Target, args) {
        const socket = new Target(...(args as [string, string | string[]]));
        sockets.push(socket);
        return socket;
      },
    });
  });

  await page.goto("/");
  await expect
    .poll(() => pageReadySockets.get(page)?.size ?? 0)
    .toBeGreaterThan(0);
  await page.evaluate(() => {
    const sockets = (globalThis as any).__packHmrSockets as WebSocket[];
    sockets.at(-1)?.close();
  });
  await expect.poll(() => pageReadySockets.get(page)?.size ?? 0).toBe(0);

  // These subscriptions are created while sendMessage has no open socket.
  // Reconnect must cause the HMR client to replay them.
  await page.locator("#load").click();
  await expect(page.locator("#a")).toHaveText("a:shared-v1");
  await expect(page.locator("#b")).toHaveText("b:shared-v1");
  await expect(page.locator("#error")).toBeEmpty();
  await expect
    .poll(() =>
      page.evaluate(
        () => ((globalThis as any).__packHmrSockets as WebSocket[]).length,
      ),
    )
    .toBeGreaterThanOrEqual(2);
});

test("applies a shared update once across two dynamic lists", async ({
  page,
}) => {
  const frames = pageFrames.get(page)!;

  await page.goto("/");
  await page.locator("#load").click();
  await expect(page.locator("#a")).toHaveText("a:shared-v1");
  await expect(page.locator("#b")).toHaveText("b:shared-v1");
  const initial = await readRuntimeState(page);
  expect(initial.evaluations).toBe(1);
  expect(initial.hmrApiAvailable).toBe(true);
  const versionedStylesheets = await page
    .locator('link[rel="stylesheet"][href*="hmr="]')
    .evaluateAll((links) => links.map((link) => new URL(link.href).pathname));
  expect(versionedStylesheets.length).toBeGreaterThan(0);
  expect(new Set(versionedStylesheets).size).toBe(versionedStylesheets.length);
  const frameStart = frames.length;

  await mutateAndWaitForBuild(page, () => writeShared("shared-dedupe-v2"));

  await expect(page.locator("#a")).toHaveText("a:shared-dedupe-v2");
  await expect(page.locator("#b")).toHaveText("b:shared-dedupe-v2");
  const updated = await readRuntimeState(page);
  expect(updated.evaluations).toBe(2);
  expect(updated.pageToken).toBe(initial.pageToken);
  expect(updated.timeOrigin).toBe(initial.timeOrigin);

  const resourcePaths = new Set<string>();
  for (const update of getTurbopackUpdates(frames.slice(frameStart))) {
    if (update.type === "partial") resourcePaths.add(update.resource.path);
  }
  expect(resourcePaths.size).toBeGreaterThanOrEqual(2);
  expect(
    getTurbopackEnvelopes(frames.slice(frameStart)).some((updates) => {
      const paths = new Set(
        updates
          .filter((update) => update.type === "partial")
          .map((update) => update.resource.path),
      );
      return paths.size >= 2;
    }),
  ).toBe(true);
});

test("swaps an older shared stylesheet when a new dynamic list joins", async ({
  page,
}) => {
  await page.goto("/");
  await page.locator("#load").click();
  await expect(page.locator("#a")).toHaveText("a:shared-v1");
  await expect(page.locator("#b")).toHaveText("b:shared-v1");
  const initial = await readRuntimeState(page);

  await mutateAndWaitForBuild(page, () =>
    fs.writeFileSync(
      indexPath,
      indexBaseline.replace(
        '[import("./a.js"), import("./b.js")]',
        '[Promise.resolve(), import("./b.js")]',
      ),
    ),
  );
  await mutateAndWaitForBuild(page, () => writeSharedCss("shared-css-swap-v2"));
  await mutateAndWaitForBuild(page, () =>
    fs.writeFileSync(indexPath, indexBaseline),
  );
  await expect
    .poll(() => readRuntimeState(page).then((state) => state.entryEvaluations))
    .toBeGreaterThanOrEqual(3);

  await page.locator("#load").click();
  await expect(page.locator("#a")).toHaveText("a:shared-v1");
  await expect(page.locator("#error")).toBeEmpty();
  await expect
    .poll(() =>
      page
        .locator('link[rel="stylesheet"]')
        .evaluateAll(
          (links) =>
            links.filter((link) =>
              new URL((link as HTMLLinkElement).href).pathname.endsWith(".css"),
            ).length,
        ),
    )
    .toBe(1);

  const updated = await readRuntimeState(page);
  expect(updated.pageToken).toBe(initial.pageToken);
  expect(updated.timeOrigin).toBe(initial.timeOrigin);
});

test("cancels an obsolete CSS reload without stalling later HMR", async ({
  page,
}) => {
  const frames = pageFrames.get(page)!;
  await page.goto("/");
  await page.locator("#load").click();
  await expect(page.locator("#a")).toHaveText("a:shared-v1");
  await expect(page.locator("#b")).toHaveText("b:shared-v1");

  let releaseCss!: () => void;
  const cssReleased = new Promise<void>((resolve) => {
    releaseCss = resolve;
  });
  let capturedCssReloads = 0;
  await page.route(/\.css(?:\?.*)?$/, async (route) => {
    if (capturedCssReloads > 0) {
      await route.continue();
      return;
    }
    capturedCssReloads += 1;
    await cssReleased;
    await route.continue();
  });

  try {
    await mutateAndWaitForBuild(page, () =>
      writeSharedCss("shared-css-reload-v2"),
    );
    await expect.poll(() => capturedCssReloads).toBe(1);

    const frameStart = frames.length;
    await mutateAndWaitForBuild(page, () =>
      fs.writeFileSync(
        indexPath,
        indexBaseline.replace(
          '[import("./a.js"), import("./b.js")]',
          "[Promise.resolve(), Promise.resolve()]",
        ),
      ),
    );
    await expect
      .poll(
        () =>
          getTurbopackUpdates(frames.slice(frameStart)).filter(
            (update) => update.type === "notFound",
          ).length,
      )
      .toBeGreaterThanOrEqual(2);

    releaseCss();
    await mutateAndWaitForBuild(page, () =>
      fs.writeFileSync(indexPath, indexBaseline),
    );
    await expect
      .poll(() =>
        readRuntimeState(page).then((state) => state.entryEvaluations),
      )
      .toBeGreaterThanOrEqual(3);

    await page.locator("#a").evaluate((element) => {
      element.textContent = "pending";
    });
    await page.locator("#b").evaluate((element) => {
      element.textContent = "pending";
    });
    await page.locator("#load").click();
    await expect(page.locator("#a")).toHaveText("a:shared-v1");
    await expect(page.locator("#b")).toHaveText("b:shared-v1");
    await expect(page.locator("#error")).toBeEmpty();
  } finally {
    releaseCss();
  }
});

test("disposes and restores missing dynamic lists without reloading", async ({
  page,
}) => {
  const receivedFrames = pageFrames.get(page)!;
  const sentFrames: string[] = [];
  page.on("websocket", (socket) => {
    socket.on("framesent", ({ payload }) => sentFrames.push(String(payload)));
  });

  await page.goto("/");
  await page.locator("#load").click();
  await expect(page.locator("#a")).toHaveText("a:shared-v1");
  await expect(page.locator("#b")).toHaveText("b:shared-v1");
  const initial = await readRuntimeState(page);

  await mutateAndWaitForBuild(page, () =>
    fs.writeFileSync(
      indexPath,
      indexBaseline.replace(
        '[import("./a.js"), import("./b.js")]',
        '[Promise.resolve(), import("./b.js")]',
      ),
    ),
  );

  await expect
    .poll(() =>
      getTurbopackUpdates(receivedFrames).find(
        (update) =>
          update.type === "notFound" &&
          /(?:^|\/)src_a_[^/]+\.js$/.test(update.resource.path),
      ),
    )
    .toBeTruthy();
  await expect
    .poll(() => readRuntimeState(page).then((state) => state.entryEvaluations))
    .toBe(2);

  await mutateAndWaitForBuild(page, () =>
    fs.writeFileSync(indexPath, indexBaseline),
  );
  await expect
    .poll(() => readRuntimeState(page).then((state) => state.entryEvaluations))
    .toBe(3);

  await page.locator("#a").evaluate((element) => {
    element.textContent = "disposed";
  });
  await page.locator("#load").click();
  await expect(page.locator("#a")).toHaveText("a:shared-v1");
  await expect(page.locator("#b")).toHaveText("b:shared-v1");
  await expect(page.locator("#error")).toBeEmpty();

  const restored = await readRuntimeState(page);
  // Both old dynamic lists disappeared during re-chunking, so the shared
  // module is released only after its final owner is gone and runs once more
  // when the restored lists load.
  expect(restored.evaluations).toBe(2);
  expect(restored.pageToken).toBe(initial.pageToken);
  expect(restored.timeOrigin).toBe(initial.timeOrigin);

  const aSubscriptions = sentFrames
    .map((frame) => {
      try {
        return JSON.parse(frame);
      } catch {
        return null;
      }
    })
    .filter(
      (message) =>
        message?.type === "turbopack-subscribe" &&
        /(?:^|\/)src_a_[^/]+\.js$/.test(message.path),
    );
  expect(aSubscriptions).toHaveLength(4);
  expect(
    aSubscriptions.filter((message) => message.validation === undefined),
  ).toHaveLength(2);
  expect(
    new Set(
      aSubscriptions
        .map((message) => message.validation)
        .filter((validation) => validation !== undefined),
    ).size,
  ).toBe(2);
});

test("rejects a dynamic bootstrap that disappears before its baseline", async ({
  page,
}) => {
  await page.addInitScript(() => {
    const originalSend = WebSocket.prototype.send;
    const delayedFrames: Array<{ socket: WebSocket; data: any }> = [];
    let delaySubscriptions = true;
    (globalThis as any).__delayedDynamicSubscriptions = [];
    (globalThis as any).__flushDynamicSubscriptions = () => {
      delaySubscriptions = false;
      for (const { socket, data } of delayedFrames.splice(0)) {
        originalSend.call(socket, data);
      }
    };
    WebSocket.prototype.send = function (data) {
      let message;
      try {
        message = JSON.parse(String(data));
      } catch {}
      if (
        delaySubscriptions &&
        message?.type === "turbopack-subscribe" &&
        /(?:^|\/)src_[ab]_[^/]+\.js$/.test(message.path)
      ) {
        const paths = (globalThis as any).__delayedDynamicSubscriptions;
        if (!paths.includes(message.path)) paths.push(message.path);
        delayedFrames.push({ socket: this, data });
        return;
      }
      originalSend.call(this, data);
    };
  });

  await page.goto("/");
  const initial = await readRuntimeState(page);
  await page.locator("#load").click();
  await expect
    .poll(() =>
      page.evaluate(
        () => (globalThis as any).__delayedDynamicSubscriptions.length,
      ),
    )
    .toBe(2);

  await mutateAndWaitForBuild(page, () =>
    fs.writeFileSync(
      indexPath,
      indexBaseline.replace(
        '[import("./a.js"), import("./b.js")]',
        "[Promise.resolve(), Promise.resolve()]",
      ),
    ),
  );
  await page.evaluate(() => (globalThis as any).__flushDynamicSubscriptions());

  await expect(page.locator("#error")).toContainText(
    "disappeared while loading",
  );
  await expect(page.locator("#a")).toHaveText("pending");
  await expect(page.locator("#b")).toHaveText("pending");
  expect((await readRuntimeState(page)).evaluations).toBeUndefined();

  await mutateAndWaitForBuild(page, () =>
    fs.writeFileSync(indexPath, indexBaseline),
  );
  await expect
    .poll(() => readRuntimeState(page).then((state) => state.entryEvaluations))
    .toBeGreaterThanOrEqual(3);
  await page.locator("#error").evaluate((element) => {
    element.textContent = "";
  });
  await page.locator("#load").click();
  await expect(page.locator("#a")).toHaveText("a:shared-v1");
  await expect(page.locator("#b")).toHaveText("b:shared-v1");
  await expect(page.locator("#error")).toBeEmpty();

  const restored = await readRuntimeState(page);
  expect(restored.evaluations).toBe(1);
  expect(restored.pageToken).toBe(initial.pageToken);
  expect(restored.timeOrigin).toBe(initial.timeOrigin);
});
