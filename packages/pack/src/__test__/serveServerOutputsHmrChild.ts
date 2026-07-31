import { execFile } from "child_process";
import fs from "fs";
import path from "path";
import { promisify } from "util";
import { serve } from "../index";

type Scenario = "dist-root" | "server-dist-root";

const execFileAsync = promisify(execFile);
const [, , projectPath, portArg, scenarioArg] = process.argv;
let compilingLogCount = 0;
const originalConsoleLog = console.log;

console.log = (...args: unknown[]) => {
  if (args[0] === "Compiling...") {
    compilingLogCount++;
  }
  originalConsoleLog(...args);
};

if (!projectPath || !portArg) {
  throw new Error(
    "Usage: serveServerOutputsHmrChild <projectPath> <port> <scenario>",
  );
}
if (scenarioArg !== "dist-root" && scenarioArg !== "server-dist-root") {
  throw new Error(`Unknown server output HMR scenario: ${scenarioArg}`);
}

const scenario: Scenario = scenarioArg;
const port = Number(portArg);
const srcDir = path.join(projectPath, "src");
const dependencyPath = path.join(srcDir, "dependency.js");
const bundlePath =
  scenario === "dist-root"
    ? path.join(projectPath, "dist", "node", "main.js")
    : path.join(projectPath, "server-output", "index.js");

function writeDependency(value: string) {
  fs.writeFileSync(
    dependencyPath,
    `export default ${JSON.stringify(value)};\n`,
  );
}

async function waitForBundleOutput(
  expected: string,
  timeoutMs = 20_000,
): Promise<string> {
  const deadline = Date.now() + timeoutMs;
  let lastOutput = "";
  let lastError: unknown;

  while (Date.now() < deadline) {
    try {
      /*
       * Execute the entry bundle instead of reading only that file: Turbopack
       * can put the changed module in a hashed chunk loaded by the entry.
       */
      const { stdout, stderr } = await execFileAsync(
        process.execPath,
        [bundlePath],
        { cwd: projectPath },
      );
      lastOutput = `${stdout}${stderr}`;
      if (lastOutput.includes(expected)) {
        return expected;
      }
    } catch (error) {
      lastError = error;
    }

    await new Promise((resolve) => setTimeout(resolve, 100));
  }

  throw new Error(
    `Timed out waiting for ${bundlePath} to print ${expected}; last output: ${lastOutput}; last error: ${String(lastError)}`,
  );
}

async function main() {
  fs.rmSync(projectPath, { recursive: true, force: true });
  fs.mkdirSync(srcDir, { recursive: true });
  writeDependency("SERVER_OUTPUT_V1");

  /*
   * Cover both locations that Project::server_changed must observe:
   *
   * - A normal Node entry is emitted directly under output.path (dist_root).
   * - A configured server entry is emitted under server.output.path
   *   (server_dist_root), which may sit outside output.path.
   */
  const config =
    scenario === "dist-root"
      ? {
          entry: [{ import: "./src/index.js", name: "main" }],
          target: "node",
          output: { path: "./dist/node", clean: true },
        }
      : {
          entry: [{ import: "./src/client.js", name: "client" }],
          output: { path: "./dist/client", clean: true },
          server: {
            entry: "./src/server.js",
            output: { path: "./server-output", filename: "index.js" },
          },
        };

  if (scenario === "dist-root") {
    fs.writeFileSync(
      path.join(srcDir, "index.js"),
      'import value from "./dependency.js";\nconsole.log(value);\n',
    );
  } else {
    fs.writeFileSync(
      path.join(srcDir, "client.js"),
      'console.log("client output");\n',
    );
    fs.writeFileSync(
      path.join(srcDir, "server.js"),
      'import value from "./dependency.js";\nconsole.log(value);\n',
    );
  }

  await serve(
    {
      config: {
        ...config,
        stats: false,
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

  const initial = await waitForBundleOutput("SERVER_OUTPUT_V1");
  const initialCompilingLogs = compilingLogCount;
  writeDependency("SERVER_OUTPUT_V2");
  const updated = await waitForBundleOutput("SERVER_OUTPUT_V2");

  console.log(
    `__SERVER_OUTPUT_HMR__${JSON.stringify({
      initial,
      initialCompilingLogs,
      scenario,
      updated,
      updateCompilingLogs: compilingLogCount - initialCompilingLogs,
    })}`,
  );
  process.kill(process.pid, "SIGTERM");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
