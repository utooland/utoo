import fs from "fs";
import os from "os";
import path from "path";
import { afterEach, describe, expect, it, vi } from "vitest";

const projectMocks = vi.hoisted(() => {
  const restart = {
    issues: [],
    resource: { headers: null, path: "entry.js" },
    type: "restart",
  };
  const project = {
    entrypointsSubscribe: () =>
      (async function* () {
        yield { apps: [], issues: [], libraries: [] };
      })(),
    hmrEvents: vi.fn(() =>
      (async function* () {
        yield restart;
      })(),
    ),
    onExit: vi.fn(async () => {}),
    shutdown: vi.fn(async () => {}),
    updateInfoSubscribe: () => (async function* () {})(),
  };

  return {
    project,
    projectFactory: vi.fn(() => async () => project),
  };
});

vi.mock("../core/project", () => ({
  projectFactory: projectMocks.projectFactory,
}));

import {
  createHotReloader,
  type HotReloaderInterface,
  type WSLike,
} from "../core/hmr";

type TestClient = WSLike & {
  messages: unknown[];
};

const tempDirs: string[] = [];
let hotReloader: HotReloaderInterface | undefined;

function createClient(): TestClient {
  const messages: unknown[] = [];
  return {
    close: vi.fn(),
    messages,
    send(data) {
      messages.push(JSON.parse(data));
    },
  };
}

afterEach(async () => {
  await hotReloader?.close();
  hotReloader = undefined;
  for (const dir of tempDirs) {
    fs.rmSync(dir, { force: true, recursive: true });
  }
  tempDirs.length = 0;
  vi.restoreAllMocks();
});

describe("HMR client isolation", () => {
  it("sends a stale subscription restart only to its client", async () => {
    vi.spyOn(console, "log").mockImplementation(() => {});
    const projectPath = fs.mkdtempSync(
      path.join(os.tmpdir(), "utoo-hmr-client-"),
    );
    tempDirs.push(projectPath);
    hotReloader = await createHotReloader(
      {
        config: {
          entry: [],
          output: { clean: true, path: "./dist" },
        },
      } as never,
      projectPath,
      projectPath,
    );

    const staleClient = createClient();
    const healthyClient = createClient();
    hotReloader.registerClient(staleClient);
    hotReloader.registerClient(healthyClient);
    staleClient.messages.length = 0;
    healthyClient.messages.length = 0;

    hotReloader.handleClientMessage(
      staleClient,
      JSON.stringify({
        path: "entry.js",
        type: "turbopack-subscribe",
        version: "stale-version",
      }),
    );

    await vi.waitFor(() => {
      expect(staleClient.messages).toContainEqual({
        action: "turbopack-message",
        data: [expect.objectContaining({ type: "restart" })],
      });
    });
    expect(healthyClient.messages).toEqual([]);
  });
});
