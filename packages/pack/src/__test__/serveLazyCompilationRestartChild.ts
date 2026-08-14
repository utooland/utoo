import fs from "fs";
import path from "path";
import { type DevServerReadyContext, serve } from "../index";

const [, , projectPath, portArg, initializeArg] = process.argv;

if (!projectPath || !portArg || !initializeArg) {
  throw new Error(
    "Usage: serveLazyCompilationRestartChild <projectPath> <port> <initialize>",
  );
}

const port = Number(portArg);
const initialize = initializeArg === "true";

async function main() {
  if (initialize) {
    fs.rmSync(projectPath, { recursive: true, force: true });
    fs.mkdirSync(path.join(projectPath, "src"), { recursive: true });
    fs.writeFileSync(
      path.join(projectPath, "src", "index.js"),
      'console.log("lazy restart fixture");\n',
    );
  }

  let readyContext: DevServerReadyContext | undefined;
  await serve(
    {
      config: {
        devServer: { lazyCompilation: true },
        entry: [{ import: "./src/index.js", name: "main" }],
        output: {
          path: "./dist",
          clean: true,
          filename: "[name].js",
        },
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

  const statuses = await Promise.all(
    readyContext.clientPaths.map(async (clientPath) => {
      const response = await fetch(
        `http://${readyContext!.hostname}:${readyContext!.port}/${clientPath.replace(/^\/+/, "")}`,
      );
      return response.status;
    }),
  );

  // Give the persistent backend enough idle time to save the session before
  // the parent starts the same project in a fresh process.
  await new Promise((resolve) => setTimeout(resolve, 1_000));
  console.log(
    `__LAZY_RESTART_RESULT__${JSON.stringify({
      clientPathCount: readyContext.clientPaths.length,
      statuses,
    })}`,
  );
  process.kill(process.pid, "SIGTERM");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
