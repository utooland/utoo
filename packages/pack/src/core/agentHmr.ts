import { createHash, randomUUID, timingSafeEqual } from "node:crypto";
import { getConnInfo } from "@hono/node-server/conninfo";
import { Hono } from "hono";

export const AGENT_HMR_PATH = "/__utoopack/agent-hmr";
const HOLD_TIMEOUT_MS = 30_000;

interface Batch {
  id: string;
  hold: Promise<void>;
  timer?: NodeJS.Timeout;
  release?: Promise<void>;
}

const batchErrorStatus = {
  alreadyOpen: 409,
  unknownBatch: 404,
  closing: 503,
} as const;

class HmrBatchError extends Error {
  constructor(
    readonly code: keyof typeof batchErrorStatus,
    message: string,
  ) {
    super(message);
  }
}

export function createAgentHmrController(
  setWatchBatchHold: (held: boolean) => Promise<void>,
) {
  let active: Batch | undefined;
  let closed = false;

  async function release(batch: Batch) {
    batch.release ??= setWatchBatchHold(false)
      .then(() => {
        clearTimeout(batch.timer);
        if (active === batch) active = undefined;
      })
      .catch((error: unknown) => {
        batch.release = undefined;
        throw error;
      });
    await batch.release;
  }

  function expire(batch: Batch) {
    if (closed || active !== batch) return;
    void release(batch).catch((error) => {
      console.error("Could not release the agent HMR batch", error);
      if (!closed && active === batch) {
        batch.timer = setTimeout(() => expire(batch), 1000).unref();
      }
    });
  }

  function checkOpen() {
    if (closed) {
      throw new HmrBatchError("closing", "Dev server is closing or closed");
    }
  }

  return {
    async begin() {
      checkOpen();
      if (active) {
        throw new HmrBatchError(
          "alreadyOpen",
          "An agent HMR batch is already open",
        );
      }
      const batch: Batch = {
        id: randomUUID(),
        hold: Promise.resolve().then(() => setWatchBatchHold(true)),
      };
      active = batch;
      try {
        await batch.hold;
      } catch (error) {
        if (active === batch) active = undefined;
        throw error;
      }
      checkOpen();
      batch.timer = setTimeout(() => expire(batch), HOLD_TIMEOUT_MS).unref();
      return batch.id;
    },
    async end(id: string) {
      checkOpen();
      const batch = active;
      if (!batch || batch.id !== id) {
        throw new HmrBatchError(
          "unknownBatch",
          "Unknown or expired agent HMR batch",
        );
      }
      await release(batch);
    },
    async close() {
      closed = true;
      const batch = active;
      if (!batch) return;
      clearTimeout(batch.timer);
      await batch.hold.catch(() => {});
      if (active === batch) await release(batch);
    },
  };
}

function hash(value: string) {
  return createHash("sha256").update(value).digest();
}

export function createAgentHmrRoutes(
  token: string,
  batches: ReturnType<typeof createAgentHmrController>,
) {
  const routes = new Hono();
  const authorization = hash(`Bearer ${token}`);

  routes.onError((error, c) => {
    if (error instanceof HmrBatchError) {
      return c.text(error.message, batchErrorStatus[error.code]);
    }
    throw error;
  });

  routes.use("*", async (c, next) => {
    const address = getConnInfo(c).remote.address;
    if (
      (address !== "::1" && !/^(?:::ffff:)?127\./.test(address ?? "")) ||
      !timingSafeEqual(hash(c.req.header("Authorization") ?? ""), authorization)
    ) {
      return c.text("Unauthorized", 401);
    }
    await next();
  });

  routes.post("/begin", async (c) => c.json({ id: await batches.begin() }));
  routes.post("/end/:id", async (c) => {
    await batches.end(c.req.param("id"));
    return c.body(null, 204);
  });

  return routes;
}
