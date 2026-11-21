import { runCommand } from "citty";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import hmr from "../commands/hmr";

const fetchMock = vi.fn<typeof fetch>();
let oldExitCode: typeof process.exitCode;
let write: ReturnType<typeof vi.spyOn>;

beforeEach(() => {
  oldExitCode = process.exitCode;
  process.exitCode = undefined;
  vi.stubEnv("UTOO_PACK_AGENT_HMR_TOKEN", "fixture-token");
  vi.stubGlobal("fetch", fetchMock);
  fetchMock.mockReset();
  write = vi.spyOn(process.stdout, "write").mockReturnValue(true);
});

afterEach(() => {
  process.exitCode = oldExitCode;
  vi.unstubAllEnvs();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const run = (...rawArgs: string[]) =>
  runCommand(hmr, { rawArgs: [...rawArgs, "--port", "3100"] });
const respond = (body: unknown, status = 200) =>
  fetchMock.mockResolvedValueOnce(
    new Response(JSON.stringify(body), { status }),
  );

describe("Node edit batch CLI", () => {
  it("prints the ID from an authenticated begin", async () => {
    respond({ batchId: "batch-a" });
    await run("begin");
    expect(write).toHaveBeenCalledWith("batch-a\n");
    expect(fetchMock).toHaveBeenCalledWith(
      "http://localhost:3100/__utoopack/agent-hmr/begin",
      expect.objectContaining({
        method: "POST",
        headers: { Authorization: "Bearer fixture-token" },
        redirect: "error",
      }),
    );
  });

  it("prints the structured committed result only after receiving it", async () => {
    const result = {
      batchId: "batch-a",
      outcome: "built",
      reason: "commit",
      watchState: "held",
    };
    respond(result);
    await run("commit", "batch-a");
    expect(write).toHaveBeenCalledWith(`${JSON.stringify(result)}\n`);
    expect(process.exitCode).toBeUndefined();
  });

  it("preserves concrete build diagnostics and exits nonzero", async () => {
    const result = {
      batchId: "batch-a",
      outcome: "failed",
      errors: [
        {
          code: "COMPILATION_FAILED",
          issues: [{ filePath: "src/app.ts", title: "Syntax error" }],
          causes: [{ name: "Error", message: "Expected expression" }],
        },
      ],
    };
    respond(result);
    await run("commit", "batch-a");
    expect(write).toHaveBeenCalledWith(`${JSON.stringify(result)}\n`);
    expect(process.exitCode).toBe(1);
  });

  it("reports expired/HOLD_TIMEOUT_RELEASED separately from compilation failure", async () => {
    const result = {
      batchId: "batch-a",
      outcome: "expired",
      error: {
        code: "HOLD_TIMEOUT_RELEASED",
        message: "Batch expired and released",
      },
    };
    respond(result);
    await run("commit", "batch-a");
    expect(write).toHaveBeenCalledWith(`${JSON.stringify(result)}\n`);
    expect(process.exitCode).toBe(1);
  });

  it("reports 202 as pending with a separate exit code, without claiming a build failure", async () => {
    respond({ batchId: "batch-a", outcome: "pending" }, 202);
    await run("commit", "batch-a");
    expect(write).toHaveBeenCalledWith(
      '{"batchId":"batch-a","outcome":"pending"}\n',
    );
    expect(process.exitCode).toBe(2);
  });

  it("waits again using GET without issuing another build", async () => {
    respond({ batchId: "batch-a", outcome: "built" });
    await run("wait", "batch-a");
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining("/wait/batch-a"),
      expect.objectContaining({ method: "GET" }),
    );
  });

  it("recovers a lost begin response by reading the active ID and waiting on the same batch", async () => {
    respond({ batchId: "batch-a" });
    respond({ batchId: "batch-a", outcome: "built" });
    await run("wait");
    expect(fetchMock.mock.calls.map(([url]) => url)).toEqual([
      "http://localhost:3100/__utoopack/agent-hmr/active",
      "http://localhost:3100/__utoopack/agent-hmr/wait/batch-a",
    ]);
    expect(
      fetchMock.mock.calls.every(([, options]) => options?.method === "GET"),
    ).toBe(true);
  });

  it("shows a concrete rejected-request code and message", async () => {
    respond({ code: "BATCH_NOT_FOUND", message: "Batch was evicted" }, 404);
    await expect(run("wait", "old-id")).rejects.toThrow(
      "BATCH_NOT_FOUND: Batch was evicted",
    );
  });

  it("does not interpret a lost connection as cancellation of an accepted build", async () => {
    fetchMock.mockRejectedValueOnce(new Error("connection lost"));
    await expect(run("commit", "batch-a")).rejects.toThrow(
      /may still be pending/,
    );
  });

  it("requires authentication before making a request", async () => {
    vi.stubEnv("UTOO_PACK_AGENT_HMR_TOKEN", "");
    await expect(run("begin")).rejects.toThrow("Set UTOO_PACK_AGENT_HMR_TOKEN");
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
