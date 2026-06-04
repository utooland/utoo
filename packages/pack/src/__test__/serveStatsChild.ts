import fs from "fs";
import path from "path";
import { serve } from "../commands/dev";

const [, , projectPath, portArg] = process.argv;

if (!projectPath || !portArg) {
  throw new Error("Usage: serveStatsChild <projectPath> <port>");
}

const port = Number(portArg);
const srcDir = path.join(projectPath, "src");
const statsPath = path.join(projectPath, "dist", "stats.json");
const htmlPath = path.join(projectPath, "dist", "index.html");

function normalizeFileName(name: string): string {
  return name.replace(/_[0-9a-f]{8}(?=\.)/g, "_<hash>");
}

function normalizeStats(stats: any) {
  return {
    assets: (stats.assets ?? [])
      .map((asset: any) => ({
        name: normalizeFileName(asset.name ?? ""),
        type: asset.type,
      }))
      .sort((a: { name: string }, b: { name: string }) =>
        a.name.localeCompare(b.name),
      ),
    entrypoints: Object.fromEntries(
      Object.entries<any>(stats.entrypoints ?? {}).map(([name, entrypoint]) => [
        name,
        {
          assets: (entrypoint.assets ?? []).map((asset: any) =>
            normalizeFileName(
              typeof asset === "string" ? asset : (asset.name ?? ""),
            ),
          ),
          chunks: (entrypoint.chunks ?? []).map((chunk: any) =>
            normalizeFileName(String(chunk)),
          ),
        },
      ]),
    ),
    htmlGenerated: fs.existsSync(htmlPath),
  };
}

async function waitForStats() {
  const deadline = Date.now() + 20_000;

  while (!fs.existsSync(statsPath)) {
    if (Date.now() > deadline) {
      throw new Error(`Timed out waiting for ${statsPath}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  const stats = JSON.parse(fs.readFileSync(statsPath, "utf8"));
  if (!stats.entrypoints?.main) {
    throw new Error("stats.json is missing the main entrypoint");
  }

  console.log(`__STATS_SNAPSHOT__${JSON.stringify(normalizeStats(stats))}`);
}

async function main() {
  fs.rmSync(projectPath, { recursive: true, force: true });
  fs.mkdirSync(srcDir, { recursive: true });
  fs.writeFileSync(
    path.join(srcDir, "index.js"),
    'console.log("serve stats snapshot");\n',
  );

  await serve(
    {
      config: {
        entry: [{ import: "./src/index.js", name: "main" }],
        html: { title: "Serve Stats Snapshot" },
        output: { path: "./dist", clean: true },
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

  await waitForStats();
  process.kill(process.pid, "SIGTERM");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
