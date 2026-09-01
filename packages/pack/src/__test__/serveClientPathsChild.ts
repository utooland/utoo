import fs from "fs";
import path from "path";
import { type DevServerReadyContext, serve } from "../index";

const [, , projectPath, portArg] = process.argv;

if (!projectPath || !portArg) {
  throw new Error("Usage: serveClientPathsChild <projectPath> <port>");
}

const port = Number(portArg);
const srcDir = path.join(projectPath, "src");
const statsPath = path.join(projectPath, "dist", "stats.json");

function normalizeFileName(name: string): string {
  return name.replace(/_(?:[0-9a-f]{8}|[0-9a-z_-]{13})(?=\.)/g, "_<hash>");
}

async function main() {
  fs.rmSync(projectPath, { recursive: true, force: true });
  fs.mkdirSync(srcDir, { recursive: true });
  fs.writeFileSync(
    path.join(srcDir, "index.js"),
    'console.log("serve client paths snapshot");\n',
  );

  let readyContext: DevServerReadyContext | undefined;

  await serve(
    {
      config: {
        entry: [{ import: "./src/index.js", name: "main" }],
        output: { path: "./dist", clean: true },
        stats: false,
      },
    },
    projectPath,
    projectPath,
    {
      hostname: "127.0.0.1",
      logServerInfo: false,
      port,
      onReady(context) {
        readyContext = context;
      },
    },
  );

  if (!readyContext) {
    throw new Error("serve onReady callback was not called");
  }

  console.log(
    `__CLIENT_PATHS_SNAPSHOT__${JSON.stringify({
      clientPaths: readyContext.clientPaths.map(normalizeFileName).sort(),
      statsGenerated: fs.existsSync(statsPath),
    })}`,
  );
  process.kill(process.pid, "SIGTERM");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
