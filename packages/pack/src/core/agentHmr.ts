import { createHash, timingSafeEqual } from "node:crypto";
import { getConnInfo } from "@hono/node-server/conninfo";
import { Hono } from "hono";
import {
  type EditBatchController,
  EditBatchRequestError,
  EditBatchWaitTimeoutError,
} from "./editBatch";

export const AGENT_HMR_PATH = "/__utoopack/agent-hmr";

function hash(value: string) {
  return createHash("sha256").update(value).digest();
}

/** Transport wait budget only: this never changes the hold or build policy. */
function waitBudget(value: string | undefined) {
  if (value === undefined) return 10_000;
  const ms = Number(value);
  if (
    !/^\d+$/.test(value) ||
    !Number.isInteger(ms) ||
    ms < 1 ||
    ms > 2_147_483_647
  ) {
    throw new EditBatchRequestError(
      "INVALID_WAIT_OPTIONS",
      "timeoutMs must be a positive timer-safe integer",
    );
  }
  return ms;
}

export function createAgentHmrRoutes(
  token: string,
  batches: EditBatchController,
) {
  const routes = new Hono();
  const authorization = hash(`Bearer ${token}`);

  routes.onError((error, c) => {
    if (error instanceof EditBatchRequestError) {
      return c.json(
        {
          code: error.code,
          message: error.message,
          ...(error.failure ? { failure: error.failure } : {}),
        },
        error.status,
      );
    }
    if (error instanceof EditBatchWaitTimeoutError) {
      return c.json({ outcome: "pending", batchId: error.batchId }, 202);
    }
    throw error;
  });

  routes.use("*", async (c, next) => {
    const address = getConnInfo(c).remote.address;
    if (
      (address !== "::1" && !/^(?:::ffff:)?127\./.test(address ?? "")) ||
      !timingSafeEqual(hash(c.req.header("Authorization") ?? ""), authorization)
    ) {
      return c.json({ code: "UNAUTHORIZED", message: "Unauthorized" }, 401);
    }
    await next();
  });

  // Recover a lost begin response without creating a second batch.
  routes.get("/active", (c) => c.json({ batchId: batches.activeId() ?? null }));
  routes.post("/begin", async (c) =>
    c.json({ batchId: await batches.begin() }),
  );
  routes.post("/commit/:id", async (c) => {
    const timeoutMs = waitBudget(c.req.query("timeoutMs"));
    const id = c.req.param("id");
    // Accepted operations always resolve with a terminal result. Disconnects only
    // detach the transport waiter; they must not cancel this server-side commit.
    batches.commit(id);
    return c.json(
      await batches.wait(id, { timeoutMs, signal: c.req.raw.signal }),
    );
  });
  routes.get("/wait/:id", async (c) => {
    const timeoutMs = waitBudget(c.req.query("timeoutMs"));
    return c.json(
      await batches.wait(c.req.param("id"), {
        timeoutMs,
        signal: c.req.raw.signal,
      }),
    );
  });

  return routes;
}
