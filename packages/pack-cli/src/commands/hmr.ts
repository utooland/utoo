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

async function request(port: string, https: boolean, path: string) {
  const portNumber = Number(port);
  if (!/^\d+$/.test(port) || portNumber < 1 || portNumber > 65535) {
    throw new Error("--port must be an integer between 1 and 65535");
  }

  const token = process.env.UTOO_PACK_AGENT_HMR_TOKEN?.trim();
  if (!token) {
    throw new Error("Set UTOO_PACK_AGENT_HMR_TOKEN to the dev server's token");
  }

  const url = `${https ? "https" : "http"}://localhost:${portNumber}${route}${path}`;
  let response: Response;
  try {
    response = await fetch(url, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
      redirect: "error",
      signal: AbortSignal.timeout(10_000),
    });
  } catch (cause) {
    throw new Error(`Could not reach the Utoopack dev server at ${url}`, {
      cause,
    });
  }

  if (!response.ok) {
    throw new Error(
      `The dev server rejected the HMR request (${response.status})`,
    );
  }
  return response;
}

const begin = defineCommand({
  meta: { name: "begin", description: "Hold HMR and print the edit batch ID" },
  args: targetArgs,
  async run({ args }) {
    const response = await request(args.port, args.https, "/begin");
    const result: unknown = await response.json();
    if (
      typeof result !== "object" ||
      result === null ||
      !("id" in result) ||
      typeof result.id !== "string" ||
      !result.id ||
      /\s/.test(result.id)
    ) {
      throw new Error("The dev server did not return an HMR batch ID");
    }
    process.stdout.write(`${result.id}\n`);
  },
});

const end = defineCommand({
  meta: { name: "end", description: "Release an edit batch" },
  args: {
    "batch-id": {
      type: "positional",
      description: "Batch ID printed by hmr begin",
    },
    ...targetArgs,
  },
  async run({ args }) {
    const response = await request(
      args.port,
      args.https,
      `/end/${encodeURIComponent(args["batch-id"])}`,
    );
    if (response.status !== 204) {
      throw new Error("The dev server did not confirm the HMR batch release");
    }
  },
});

export default defineCommand({
  meta: {
    name: "hmr",
    description: "Control edit batches (requires UTOO_PACK_AGENT_HMR_TOKEN)",
  },
  subCommands: { begin, end },
});
