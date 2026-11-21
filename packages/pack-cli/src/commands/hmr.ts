import { defineCommand } from "citty";

const route = "/__utoopack/agent-hmr";
const targetArgs = {
  port: {
    type: "string",
    required: true,
    description: "Port of the running Utoopack dev server",
  },
  https: {
    type: "boolean",
    description: "Connect to a dev server running HTTPS",
  },
} as const;

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

async function request(
  port: string,
  https: boolean,
  path: string,
  method: "GET" | "POST",
) {
  const portNumber = Number(port);
  if (!/^\d+$/.test(port) || portNumber < 1 || portNumber > 65535) {
    throw new Error("--port must be an integer between 1 and 65535");
  }
  const token = process.env.UTOO_PACK_AGENT_HMR_TOKEN?.trim();
  if (!token)
    throw new Error("Set UTOO_PACK_AGENT_HMR_TOKEN to the dev server's token");
  const url = `${https ? "https" : "http"}://localhost:${portNumber}${route}${path}`;
  let response: Response;
  try {
    response = await fetch(url, {
      method,
      headers: { Authorization: `Bearer ${token}` },
      redirect: "error",
      // The server's 10s transport wait budget returns 202 first. A lost
      // connection never means the accepted server-side commit was cancelled.
      signal: AbortSignal.timeout(15_000),
    });
  } catch (cause) {
    throw new Error(
      `Could not complete the request to ${url}; an accepted batch may still be pending. Reconnect with hmr wait.`,
      { cause },
    );
  }
  const result: unknown = await response.json();
  if (!response.ok) {
    const detail =
      isObject(result) && typeof result.message === "string"
        ? result.message
        : `HTTP ${response.status}`;
    const code =
      isObject(result) && typeof result.code === "string"
        ? `${result.code}: `
        : "";
    throw new Error(`${code}${detail}`);
  }
  return { response, result };
}

function batchId(result: unknown) {
  if (
    !isObject(result) ||
    typeof result.batchId !== "string" ||
    !result.batchId ||
    /\s/.test(result.batchId)
  ) {
    throw new Error("The dev server did not return an edit batch ID");
  }
  return result.batchId;
}

function printResult(result: unknown, status: number) {
  if (!isObject(result) || typeof result.batchId !== "string")
    throw new Error("Invalid edit batch result");
  if (status === 202 && result.outcome === "pending") {
    process.stdout.write(`${JSON.stringify(result)}\n`);
    process.exitCode = 2;
    return;
  }
  if (
    status !== 200 ||
    !["built", "failed", "expired"].includes(String(result.outcome))
  ) {
    throw new Error(
      "The dev server did not return an edit batch terminal result",
    );
  }
  // Preserve every native issue and I/O/HTML/publish cause in machine-readable output.
  process.stdout.write(`${JSON.stringify(result)}\n`);
  if (result.outcome !== "built") process.exitCode = 1;
}

const begin = defineCommand({
  meta: {
    name: "begin",
    description: "Hold watcher edit batches and print the batch ID",
  },
  args: targetArgs,
  async run({ args }) {
    const { result } = await request(args.port, args.https, "/begin", "POST");
    process.stdout.write(`${batchId(result)}\n`);
  },
});

const commit = defineCommand({
  meta: {
    name: "commit",
    description: "Commit an edit batch and wait for its output result",
  },
  args: {
    "batch-id": {
      type: "positional",
      required: true,
      description: "Batch ID printed by hmr begin",
    },
    ...targetArgs,
  },
  async run({ args }) {
    const { response, result } = await request(
      args.port,
      args.https,
      `/commit/${encodeURIComponent(args["batch-id"])}`,
      "POST",
    );
    printResult(result, response.status);
  },
});

const wait = defineCommand({
  meta: {
    name: "wait",
    description:
      "Wait again without rebuilding; omit ID to recover the active batch",
  },
  args: {
    "batch-id": {
      type: "positional",
      required: false,
      description: "Existing ID, or the active batch if omitted",
    },
    ...targetArgs,
  },
  async run({ args }) {
    const id =
      args["batch-id"] ||
      batchId((await request(args.port, args.https, "/active", "GET")).result);
    const { response, result } = await request(
      args.port,
      args.https,
      `/wait/${encodeURIComponent(id)}`,
      "GET",
    );
    printResult(result, response.status);
  },
});

export default defineCommand({
  meta: {
    name: "hmr",
    description:
      "Control Node watcher edit batches (requires UTOO_PACK_AGENT_HMR_TOKEN)",
  },
  subCommands: { begin, commit, wait },
});
