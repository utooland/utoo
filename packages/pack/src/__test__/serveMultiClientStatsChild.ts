import fs from "fs";
import path from "path";
import { serve } from "../commands/dev";

const [, , projectPath, portArg] = process.argv;

if (!projectPath || !portArg) {
  throw new Error("Usage: serveMultiClientStatsChild <projectPath> <port>");
}

const port = Number(portArg);
const srcDir = path.join(projectPath, "src");
const statsPath = path.join(projectPath, "dist", "stats.json");

function entrypointAssets(stats: any, name: string): string[] {
  return (stats.entrypoints?.[name]?.assets ?? []).map((asset: any) =>
    typeof asset === "string" ? asset : (asset?.name ?? ""),
  );
}

function chunkLists(assets: string[]): string[] {
  return assets.filter(
    (asset) => asset.includes("src_alpha_") || asset.includes("src_beta_"),
  );
}

async function waitForStats() {
  const deadline = Date.now() + 20_000;

  while (true) {
    if (fs.existsSync(statsPath)) {
      const stats = JSON.parse(fs.readFileSync(statsPath, "utf8"));
      if (stats.entrypoints?.alpha && stats.entrypoints?.beta) {
        return stats;
      }
    }
    if (Date.now() > deadline) {
      throw new Error(`Timed out waiting for ${statsPath}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

async function main() {
  fs.rmSync(projectPath, { recursive: true, force: true });
  fs.mkdirSync(srcDir, { recursive: true });
  fs.writeFileSync(
    path.join(srcDir, "alpha.js"),
    'import("./alpha-lazy.js").then(({ default: value }) => console.log(value));\n',
  );
  fs.writeFileSync(
    path.join(srcDir, "alpha-lazy.js"),
    'export default "alpha";\n',
  );
  fs.writeFileSync(
    path.join(srcDir, "beta.js"),
    'import("./beta-lazy.js").then(({ default: value }) => console.log(value));\n',
  );
  fs.writeFileSync(
    path.join(srcDir, "beta-lazy.js"),
    'export default "beta";\n',
  );

  await serve(
    {
      config: {
        entry: [
          { import: "./src/alpha.js", name: "alpha" },
          { import: "./src/beta.js", name: "beta" },
        ],
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

  const stats = await waitForStats();
  const alphaChunkLists = chunkLists(entrypointAssets(stats, "alpha"));
  const betaChunkLists = chunkLists(entrypointAssets(stats, "beta"));

  console.log(
    `__STATS_SNAPSHOT__${JSON.stringify({
      alphaHasOwnChunkLists: alphaChunkLists.some((asset) =>
        asset.includes("src_alpha_"),
      ),
      alphaHasOnlyOwnChunkLists: alphaChunkLists.every((asset) =>
        asset.includes("src_alpha_"),
      ),
      betaHasOwnChunkLists: betaChunkLists.some((asset) =>
        asset.includes("src_beta_"),
      ),
      betaHasOnlyOwnChunkLists: betaChunkLists.every((asset) =>
        asset.includes("src_beta_"),
      ),
    })}`,
  );
  process.kill(process.pid, "SIGTERM");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
