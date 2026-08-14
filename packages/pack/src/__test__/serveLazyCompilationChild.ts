import fs from "fs";
import path from "path";
import { WebSocket } from "ws";
import { type DevServerReadyContext, serve } from "../index";

const [, , projectPath, portArg] = process.argv;

if (!projectPath || !portArg) {
  throw new Error("Usage: serveLazyCompilationChild <projectPath> <port>");
}

const port = Number(portArg);
const srcDir = path.join(projectPath, "src");
const publicDir = path.join(projectPath, "public");
const distDir = path.join(projectPath, "dist");
const marker = "__LAZY_COMPILATION_MARKER__";
const markerV1 = `${marker}_V1`;
const markerV2 = `${marker}_V2`;
const copiedAssetMarker = "__COPIED_ASSET_MARKER__";
const copiedAssetMarkerV2 = `${copiedAssetMarker}_V2`;
const loaderInvocationPath = path.join(projectPath, "lazy-loader-invoked.txt");

function delay(duration: number) {
  return new Promise((resolve) => setTimeout(resolve, duration));
}

function waitForWebSocketOpen(socket: WebSocket) {
  return new Promise<void>((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error("Timed out opening the HMR WebSocket"));
    }, 5_000);

    socket.once("open", () => {
      clearTimeout(timeout);
      resolve();
    });
    socket.once("error", (error) => {
      clearTimeout(timeout);
      reject(error);
    });
  });
}

function waitForHmrUpdate(
  socket: WebSocket,
  resourcePath: string,
  expectedMarker: string,
) {
  return new Promise<{
    hmrPartialUpdateReceived: boolean;
    hmrReloadReceived: boolean;
  }>((resolve, reject) => {
    let settled = false;
    let hmrReloadReceived = false;
    const timeout = setTimeout(() => {
      cleanup();
      reject(
        new Error(
          `Timed out waiting for a partial HMR update for ${resourcePath}`,
        ),
      );
    }, 10_000);

    const cleanup = () => {
      clearTimeout(timeout);
      socket.off("error", onError);
      socket.off("message", onMessage);
    };
    const finish = (hmrPartialUpdateReceived: boolean) => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      resolve({ hmrPartialUpdateReceived, hmrReloadReceived });
    };
    const onError = (error: Error) => {
      if (settled) {
        return;
      }
      settled = true;
      cleanup();
      reject(error);
    };
    const onMessage = (data: WebSocket.RawData) => {
      const message = JSON.parse(data.toString());
      if (message.action === "reload") {
        hmrReloadReceived = true;
        finish(false);
        return;
      }
      if (message.action !== "turbopack-message") {
        return;
      }

      const updates = Array.isArray(message.data)
        ? message.data
        : [message.data];
      const receivedExpectedUpdate = updates.some(
        (update) =>
          update?.type === "partial" &&
          update.resource?.path === resourcePath &&
          JSON.stringify(update).includes(expectedMarker),
      );
      if (receivedExpectedUpdate) {
        // Give an incorrectly scheduled full reload a chance to arrive before
        // declaring the HMR exchange successful.
        setTimeout(() => finish(true), 50);
      }
    };

    socket.on("error", onError);
    socket.on("message", onMessage);
  });
}

function directoryContainsMarker(directory: string): boolean {
  if (!fs.existsSync(directory)) {
    return false;
  }

  return fs.readdirSync(directory, { withFileTypes: true }).some((entry) => {
    const entryPath = path.join(directory, entry.name);
    return entry.isDirectory()
      ? directoryContainsMarker(entryPath)
      : fs.readFileSync(entryPath).includes(marker);
  });
}

async function main() {
  fs.rmSync(projectPath, { recursive: true, force: true });
  fs.mkdirSync(srcDir, { recursive: true });
  fs.mkdirSync(publicDir, { recursive: true });
  fs.writeFileSync(
    path.join(srcDir, "index.js"),
    'import("./lazy.lazy.js").then(({ default: value }) => console.log(value));\n',
  );
  fs.writeFileSync(
    path.join(srcDir, "lazy.lazy.js"),
    `export default "${markerV1}";\n`,
  );
  const lazyLoaderPath = path.join(projectPath, "lazy-loader.cjs");
  fs.writeFileSync(
    lazyLoaderPath,
    `const fs = require("fs");\nmodule.exports = function(source) { fs.writeFileSync(${JSON.stringify(
      loaderInvocationPath,
    )}, "invoked"); return source; };\n`,
  );
  fs.writeFileSync(path.join(publicDir, "copied.txt"), copiedAssetMarker);

  let readyContext: DevServerReadyContext | undefined;

  await serve(
    {
      config: {
        devServer: {
          lazyCompilation: true,
        },
        entry: [{ import: "./src/index.js", name: "main" }],
        module: {
          rules: {
            "*.lazy.js": [lazyLoaderPath],
          },
        },
        output: {
          path: "./dist",
          clean: true,
          filename: "[name].js",
          chunkFilename: "[name].js",
          copy: [{ from: "./public", to: "static" }],
        },
        sourceMaps: true,
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

  const lazyMaterializedAtReady = directoryContainsMarker(distDir);
  const lazyLoaderInvokedAtReady = fs.existsSync(loaderInvocationPath);
  const copiedAssetMaterializedAtReady = fs.existsSync(
    path.join(distDir, "static", "copied.txt"),
  );
  const copiedAssetResponse = await fetch(
    `http://${readyContext.hostname}:${readyContext.port}/static/copied.txt`,
  );
  const copiedAssetBody = await copiedAssetResponse.text();
  fs.writeFileSync(path.join(publicDir, "copied.txt"), copiedAssetMarkerV2);
  let copiedAssetUpdateObserved = false;
  for (let attempt = 0; attempt < 50; attempt++) {
    const response = await fetch(
      `http://${readyContext.hostname}:${readyContext.port}/static/copied.txt`,
    );
    if (
      response.status === 200 &&
      (await response.text()) === copiedAssetMarkerV2
    ) {
      copiedAssetUpdateObserved = true;
      break;
    }
    await delay(100);
  }
  const pendingPaths = [...readyContext.clientPaths];
  const entryPaths = new Set(readyContext.clientPaths);
  let entryResponsesSucceeded = true;
  let expandedRoutesSurvivedEviction = false;
  const requestedPaths = new Set<string>();
  const responses: Array<{ body: string; path: string; status: number }> = [];
  while (pendingPaths.length > 0 && requestedPaths.size < 32) {
    const clientPath = pendingPaths.shift()!;
    if (requestedPaths.has(clientPath)) {
      continue;
    }
    requestedPaths.add(clientPath);

    const requestPath = `/${clientPath.replace(/^\/+/, "")}`;
    const response = await fetch(
      `http://${readyContext.hostname}:${readyContext.port}${requestPath}`,
    );
    const body = await response.text();
    responses.push({ body, path: clientPath, status: response.status });
    if (entryPaths.has(clientPath) && response.status !== 200) {
      entryResponsesSucceeded = false;
    }
    for (const referencedPath of body.match(
      /(?:[A-Za-z0-9_.-]+\/)*[A-Za-z0-9_.-]+\.js/g,
    ) ?? []) {
      if (!requestedPaths.has(referencedPath)) {
        pendingPaths.push(referencedPath);
      }
    }

    if (
      !expandedRoutesSurvivedEviction &&
      [...entryPaths].every((entryPath) => requestedPaths.has(entryPath))
    ) {
      // The lazy graph stores the routes exposed by serving an asset. Let a
      // forced idle snapshot evict everything it is allowed to before asking
      // for the newly exposed dynamic assets.
      await delay(5_000);
      expandedRoutesSurvivedEviction = true;
    }
  }

  const entryResponseContainsLazyMarker = responses.some(
    ({ body, path: responsePath }) =>
      entryPaths.has(responsePath) && body.includes(marker),
  );
  const lazyResponses = responses.filter(({ body }) => body.includes(markerV1));
  const dynamicHmrResponse = responses.find(
    ({ body }) =>
      /source:\s*["']dynamic["']/.test(body) &&
      lazyResponses.some(({ path: lazyPath }) => body.includes(lazyPath)),
  );
  if (!dynamicHmrResponse) {
    throw new Error(
      `No dynamic HMR chunk list was found in responses: ${[
        ...requestedPaths,
      ].join(", ")}`,
    );
  }

  const rangedAsset = lazyResponses.find(({ status }) => status === 200);
  if (!rangedAsset) {
    throw new Error(
      "No successful lazy asset response was available for Range",
    );
  }
  const rangedResponse = await fetch(
    `http://${readyContext.hostname}:${readyContext.port}/${rangedAsset.path.replace(/^\/+/, "")}`,
    { headers: { Range: "bytes=0-9" } },
  );
  const rangedBody = await rangedResponse.arrayBuffer();
  const expectedContentRange = `bytes 0-9/${Buffer.byteLength(rangedAsset.body)}`;
  const headResponse = await fetch(
    `http://${readyContext.hostname}:${readyContext.port}/${rangedAsset.path.replace(/^\/+/, "")}`,
    { headers: { Range: "bytes=0-9" }, method: "HEAD" },
  );
  const headBody = await headResponse.arrayBuffer();

  const sourceMapCandidates = lazyResponses.flatMap((response) =>
    [...response.body.matchAll(/sourceMappingURL=([^\s"'`]+)/g)].map(
      (match) => ({ owner: response, reference: match[1] }),
    ),
  );
  const sourceMapCandidate = sourceMapCandidates.find(({ reference }) =>
    /\.map(?:\?|$)/.test(reference),
  );
  if (!sourceMapCandidate) {
    throw new Error(
      `No external source map reference was found: ${JSON.stringify(
        sourceMapCandidates.map(({ reference }) => reference),
      )}`,
    );
  }
  const { owner: sourceMapOwner, reference: sourceMapReference } =
    sourceMapCandidate;
  const sourceMapPath = sourceMapReference.startsWith("/")
    ? sourceMapReference
    : path.posix.join(
        path.posix.dirname(sourceMapOwner.path),
        sourceMapReference,
      );
  const sourceMapResponse = await fetch(
    `http://${readyContext.hostname}:${readyContext.port}/${sourceMapPath.replace(/^\/+/, "")}`,
  );
  const sourceMapBody = await sourceMapResponse.text();

  const socket = new WebSocket(
    `ws://${readyContext.hostname}:${readyContext.port}/turbopack-hmr`,
  );
  let hmrResult: Awaited<ReturnType<typeof waitForHmrUpdate>>;
  try {
    await waitForWebSocketOpen(socket);
    const update = waitForHmrUpdate(socket, dynamicHmrResponse.path, markerV2);
    socket.send(
      JSON.stringify({
        type: "turbopack-subscribe",
        path: dynamicHmrResponse.path,
      }),
    );
    // The protocol deliberately does not send an acknowledgement after the
    // server consumes the subscription's initial snapshot.
    await delay(1_000);
    fs.writeFileSync(
      path.join(srcDir, "lazy.lazy.js"),
      `export default "${markerV2}";\n`,
    );
    hmrResult = await update;
  } finally {
    socket.close();
  }

  console.log(
    `__LAZY_COMPILATION_RESULT__${JSON.stringify({
      copiedAssetMaterializedAtReady,
      copiedAssetResponseContainsMarker: copiedAssetBody === copiedAssetMarker,
      copiedAssetResponseStatus: copiedAssetResponse.status,
      copiedAssetUpdateObserved,
      entryResponsesSucceeded,
      entryResponseContainsLazyMarker,
      expandedRoutesSurvivedEviction,
      hmrChunkListPathDiscovered: Boolean(dynamicHmrResponse.path),
      headResponseContentLength:
        headResponse.headers.get("content-length") ===
        Buffer.byteLength(rangedAsset.body).toString(),
      headResponseLength: headBody.byteLength,
      headResponseStatus: headResponse.status,
      ...hmrResult,
      lazyMaterializedAtReady,
      lazyLoaderInvokedAtReady,
      lazyResponseContainsMarker: lazyResponses.some(({ body }) =>
        body.includes(markerV1),
      ),
      lazyResponseStatus: lazyResponses.every(({ status }) => status === 200)
        ? 200
        : Math.max(...lazyResponses.map(({ status }) => status)),
      rangedResponseContentRange:
        rangedResponse.headers.get("content-range") === expectedContentRange,
      rangedResponseLength: rangedBody.byteLength,
      rangedResponseStatus: rangedResponse.status,
      sourceMapResponseIsJson: (() => {
        try {
          JSON.parse(sourceMapBody);
          return true;
        } catch {
          return false;
        }
      })(),
      sourceMapResponseStatus: sourceMapResponse.status,
    })}`,
  );
  process.kill(process.pid, "SIGTERM");
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
