import fs from "fs";
import path from "path";
import { serve } from "../commands/dev";

const [, , projectPath, portArg] = process.argv;

if (!projectPath || !portArg) {
  throw new Error("Usage: serveMultiServerStatsChild <projectPath> <port>");
}

const port = Number(portArg);
const srcDir = path.join(projectPath, "src");
const statsPath = path.join(projectPath, "dist/server/stats.json");
const expectedEntries = ["detail-server", "index-server", "server"];

function entryNames(stats: any): string[] {
  return Object.keys(stats.entrypoints ?? {}).sort();
}

function entryAsset(stats: any, name: string): string | undefined {
  const assets = stats.entrypoints?.[name]?.assets ?? [];
  return assets
    .map((asset: any) =>
      typeof asset === "string" ? asset : (asset?.name ?? ""),
    )
    .find(
      (asset: string) =>
        asset.endsWith(".js") &&
        new RegExp(`(?:^|/)${name}\\.[a-f0-9]{8}\\.js$`).test(asset),
    );
}

function sharedAsset(stats: any): string | undefined {
  const assetsByEntry = expectedEntries.map((name) =>
    (stats.entrypoints?.[name]?.assets ?? []).map((asset: any) =>
      typeof asset === "string" ? asset : (asset?.name ?? ""),
    ),
  );
  return assetsByEntry[0]?.find(
    (asset: string) =>
      asset.endsWith(".js") &&
      assetsByEntry.every((assets) => assets.includes(asset)),
  );
}

async function waitForStats(predicate: (stats: any) => boolean): Promise<any> {
  const deadline = Date.now() + 20_000;

  while (Date.now() <= deadline) {
    try {
      const stats = JSON.parse(fs.readFileSync(statsPath, "utf8"));
      if (predicate(stats)) {
        return stats;
      }
    } catch {
      // The initial build may not have written stats yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  throw new Error(`Timed out waiting for updated server stats at ${statsPath}`);
}

async function main() {
  fs.rmSync(projectPath, { recursive: true, force: true });
  fs.mkdirSync(srcDir, { recursive: true });
  fs.writeFileSync(path.join(srcDir, "app.ts"), 'console.log("client");\n');
  fs.writeFileSync(
    path.join(srcDir, "shared.ts"),
    'export const shared = "shared";\n',
  );
  fs.writeFileSync(
    path.join(srcDir, "server.ts"),
    'import { shared } from "./shared"; console.log("server", shared);\n',
  );
  fs.writeFileSync(
    path.join(srcDir, "index.server.ts"),
    'import { shared } from "./shared"; console.log("index", shared);\n',
  );
  const detailPath = path.join(srcDir, "detail.server.ts");
  fs.writeFileSync(
    detailPath,
    'import { shared } from "./shared"; console.log("detail v1", shared);\n',
  );

  await serve(
    {
      config: {
        entry: [{ import: "./src/app.ts", name: "app" }],
        output: { path: "./dist/client", clean: true },
        server: {
          entry: { name: "server", import: "./src/server.ts" },
          entries: [
            { name: "index-server", import: "./src/index.server.ts" },
            { name: "detail-server", import: "./src/detail.server.ts" },
          ],
          output: {
            path: "./dist/server",
            filename: "[name].[contenthash:8].js",
          },
        },
        stats: true,
      },
    },
    projectPath,
    projectPath,
    {
      hostname: "127.0.0.1",
      logServerInfo: false,
      port,
    },
  );

  const initial = await waitForStats(
    (stats) =>
      JSON.stringify(entryNames(stats)) === JSON.stringify(expectedEntries) &&
      Boolean(sharedAsset(stats)),
  );
  const initialDetailAsset = entryAsset(initial, "detail-server");
  const initialSharedAsset = sharedAsset(initial);
  if (!initialDetailAsset) {
    throw new Error("Initial stats are missing the detail-server JS asset");
  }

  fs.writeFileSync(
    detailPath,
    'import { shared } from "./shared"; console.log("detail v2", shared);\n',
  );
  const rebuilt = await waitForStats(
    (stats) => entryAsset(stats, "detail-server") !== initialDetailAsset,
  );
  const rebuiltDetailAsset = entryAsset(rebuilt, "detail-server");
  const rebuiltSharedAsset = sharedAsset(rebuilt);
  const entryAssetsBeforeSharedChange = Object.fromEntries(
    expectedEntries.map((name) => [name, entryAsset(rebuilt, name)]),
  );

  fs.writeFileSync(
    path.join(srcDir, "shared.ts"),
    'export const shared = "shared v2";\n',
  );
  const sharedRebuilt = await waitForStats(
    (stats) => sharedAsset(stats) !== rebuiltSharedAsset,
  );
  const rebuiltEntries = entryNames(sharedRebuilt);

  console.log(
    `__STATS_SNAPSHOT__${JSON.stringify({
      changedEntry: rebuiltDetailAsset !== initialDetailAsset,
      initialEntries: entryNames(initial),
      preservedSharedAsset:
        Boolean(initialSharedAsset) &&
        sharedAsset(rebuilt) === initialSharedAsset,
      preservedEntries:
        JSON.stringify(rebuiltEntries) === JSON.stringify(expectedEntries),
      rebuiltEntries,
      sharedChangeInvalidatedEntries: expectedEntries.every(
        (name) =>
          entryAsset(sharedRebuilt, name) !==
          entryAssetsBeforeSharedChange[name],
      ),
    })}`,
  );
  process.kill(process.pid, "SIGTERM");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
