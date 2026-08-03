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

function entryAssets(stats: any, name: string): string[] {
  return (stats.entrypoints?.[name]?.assets ?? [])
    .map((asset: any) =>
      typeof asset === "string" ? asset : (asset?.name ?? ""),
    )
    .filter((asset: string) => asset.endsWith(".js"));
}

function sharedAsset(
  stats: any,
  includedEntries: string[],
  excludedEntries: string[] = [],
): string | undefined {
  const includedAssets = includedEntries.map((name) =>
    entryAssets(stats, name),
  );
  return includedAssets[0]?.find(
    (asset: string) =>
      /(?:^|\/)chunks\/server-shared/.test(asset) &&
      includedAssets.every((assets) => assets.includes(asset)) &&
      excludedEntries.every(
        (name) => !entryAssets(stats, name).includes(asset),
      ),
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
    'import { sharedAll } from "./shared-all"; export const shared = `shared by primary entries using ${sharedAll}`;\n',
  );
  fs.writeFileSync(
    path.join(srcDir, "shared-all.ts"),
    'export const sharedAll = "shared by all entries";\n',
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
    'import { sharedAll } from "./shared-all"; console.log("detail v1", sharedAll);\n',
  );

  await serve(
    {
      config: {
        entry: [{ import: "./src/app.ts", name: "app" }],
        output: { path: "./dist/client", clean: true },
        server: {
          entry: [
            { name: "server", import: "./src/server.ts" },
            { name: "index-server", import: "./src/index.server.ts" },
            { name: "detail-server", import: "./src/detail.server.ts" },
          ],
          output: {
            path: "./dist/server",
            filename: "entries/[name].[contenthash:8].js",
            chunkFilename: "chunks/[name].[contenthash:8].js",
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
      Boolean(sharedAsset(stats, expectedEntries)) &&
      Boolean(
        sharedAsset(stats, ["server", "index-server"], ["detail-server"]),
      ),
  );
  const initialDetailAsset = entryAsset(initial, "detail-server");
  const initialAllEntrySharedAsset = sharedAsset(initial, expectedEntries);
  const initialPrimarySharedAsset = sharedAsset(
    initial,
    ["server", "index-server"],
    ["detail-server"],
  );
  if (!initialDetailAsset) {
    throw new Error("Initial stats are missing the detail-server JS asset");
  }

  fs.writeFileSync(
    detailPath,
    'import { sharedAll } from "./shared-all"; console.log("detail v2", sharedAll);\n',
  );
  const rebuilt = await waitForStats(
    (stats) => entryAsset(stats, "detail-server") !== initialDetailAsset,
  );
  const rebuiltDetailAsset = entryAsset(rebuilt, "detail-server");
  const rebuiltAllEntrySharedAsset = sharedAsset(rebuilt, expectedEntries);
  const rebuiltPrimarySharedAsset = sharedAsset(
    rebuilt,
    ["server", "index-server"],
    ["detail-server"],
  );
  const entryAssetsBeforeSharedChange = Object.fromEntries(
    expectedEntries.map((name) => [name, entryAsset(rebuilt, name)]),
  );

  fs.writeFileSync(
    path.join(srcDir, "shared.ts"),
    'import { sharedAll } from "./shared-all"; export const shared = `shared by primary entries v2 using ${sharedAll}`;\n',
  );
  const sharedRebuilt = await waitForStats(
    (stats) =>
      sharedAsset(stats, ["server", "index-server"], ["detail-server"]) !==
      rebuiltPrimarySharedAsset,
  );
  const rebuiltEntries = entryNames(sharedRebuilt);

  console.log(
    `__STATS_SNAPSHOT__${JSON.stringify({
      changedEntry: rebuiltDetailAsset !== initialDetailAsset,
      initialEntries: entryNames(initial),
      preservedSharedAssets:
        Boolean(initialAllEntrySharedAsset) &&
        Boolean(initialPrimarySharedAsset) &&
        rebuiltAllEntrySharedAsset === initialAllEntrySharedAsset &&
        rebuiltPrimarySharedAsset === initialPrimarySharedAsset,
      preservedEntries:
        JSON.stringify(rebuiltEntries) === JSON.stringify(expectedEntries),
      rebuiltEntries,
      sharedChangeInvalidatedAffectedEntries: ["server", "index-server"].every(
        (name) =>
          entryAsset(sharedRebuilt, name) !==
          entryAssetsBeforeSharedChange[name],
      ),
      sharedChangePreservedUnaffectedEntry:
        entryAsset(sharedRebuilt, "detail-server") ===
        entryAssetsBeforeSharedChange["detail-server"],
    })}`,
  );
  process.kill(process.pid, "SIGTERM");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
