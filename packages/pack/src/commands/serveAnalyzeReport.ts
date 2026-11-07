import { createReadStream } from "node:fs";
import { realpath, stat } from "node:fs/promises";
import { createServer } from "node:http";
import path from "node:path";

export interface AnalyzeServer {
  url: string;
  close(): Promise<void>;
}

const contentTypes: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
};

/** Serve an exported report on loopback, without keeping a compiler alive. */
export async function serveAnalyzeReport(
  outputPath: string,
  port = 4000,
): Promise<AnalyzeServer> {
  if (!Number.isInteger(port) || port < 0 || port > 65535) {
    throw new Error("Analyzer port must be an integer between 0 and 65535.");
  }
  const root = await realpath(outputPath);
  const server = createServer(async (request, response) => {
    if (request.method !== "GET" && request.method !== "HEAD") {
      response.writeHead(405, { Allow: "GET, HEAD" }).end();
      return;
    }
    try {
      const pathname = decodeURIComponent(
        new URL(request.url || "/", "http://localhost").pathname,
      );
      const filename = await realpath(
        path.resolve(root, `.${pathname === "/" ? "/index.html" : pathname}`),
      );
      const relative = path.relative(root, filename);
      if (
        relative === ".." ||
        relative.startsWith(`..${path.sep}`) ||
        path.isAbsolute(relative)
      ) {
        response.writeHead(404).end();
        return;
      }
      const info = await stat(filename);
      if (!info.isFile()) {
        response.writeHead(404).end();
        return;
      }
      response.writeHead(200, {
        "Content-Type":
          contentTypes[path.extname(filename)] || "application/octet-stream",
        "Content-Length": info.size,
        "Cache-Control": "no-cache",
        "X-Content-Type-Options": "nosniff",
      });
      if (request.method === "HEAD") {
        response.end();
      } else {
        const stream = createReadStream(filename);
        stream.on("error", () => response.destroy());
        response.on("close", () => stream.destroy());
        stream.pipe(response);
      }
    } catch {
      response.writeHead(404).end();
    }
  });
  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => {
      server.off("error", reject);
      resolve();
    });
  });
  const address = server.address();
  if (!address || typeof address === "string") {
    server.close();
    throw new Error("Unable to determine Analyzer server address.");
  }
  return {
    url: `http://127.0.0.1:${address.port}`,
    close: () =>
      new Promise<void>((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
      }),
  };
}
