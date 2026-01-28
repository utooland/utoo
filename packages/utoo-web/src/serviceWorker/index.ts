import { SWMessageType } from "../message";
import { Project } from "../project/Project";
import { ProjectEndpoint } from "../types";

declare let self: ServiceWorkerGlobalScope;

const HANDSHAKE_TIMEOUT = 10000; // 10 seconds
const HANDSHAKE_WARNING_INTERVAL = 3000; // 3 seconds

let _resolve: () => void;
let _handshakeCompleted = false;

let _promise: Promise<void> = new Promise((resolve) => {
  _resolve = () => {
    _handshakeCompleted = true;
    resolve();
  };
});

let _projectEndpoint: ProjectEndpoint;

const params = new URLSearchParams(self.location.search);
const _serviceWorkerScope = params.get("scope");
const _targetDirToCwd = params.get("targetDirToCwd");

self.addEventListener("install", (event) => {
  event.waitUntil(self.skipWaiting());
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("message", (event) => {
  if (event.data && event.data[SWMessageType.HandShake] === true) {
    console.log("[ServiceWorker] Handshake message received");
    _projectEndpoint = Project.fork(
      new MessageChannel(),
      event.source as Client,
    );
    _resolve();
    console.log("[ServiceWorker] Handshake completed, ProjectEndpoint ready");
  } else if (event.data && event.data[SWMessageType.HeartbeatPing] === true) {
    (event.source as Client).postMessage({
      [SWMessageType.HeartbeatPong]: true,
    });
  }
});

async function waitForHandshake(requestUrl: string): Promise<void> {
  if (_handshakeCompleted) return;

  const startTime = Date.now();
  const warningTimer = setInterval(() => {
    const elapsed = Date.now() - startTime;
    console.warn(
      `[ServiceWorker] Still waiting for handshake... (${elapsed}ms elapsed, request: ${requestUrl})`,
    );
  }, HANDSHAKE_WARNING_INTERVAL);

  const timeoutPromise = new Promise<never>((_, reject) => {
    setTimeout(() => {
      reject(
        new Error(
          `[ServiceWorker] Handshake timeout after ${HANDSHAKE_TIMEOUT}ms. ` +
            `Make sure the main thread calls installServiceWorker() and sends the handshake message. ` +
            `Request: ${requestUrl}`,
        ),
      );
    }, HANDSHAKE_TIMEOUT);
  });

  try {
    await Promise.race([_promise, timeoutPromise]);
  } finally {
    clearInterval(warningTimer);
  }
}

self.addEventListener("fetch", (event: FetchEvent) => {
  let { url: urlStr } = event.request;
  let url = new URL(urlStr);
  if (typeof _serviceWorkerScope === "string") {
    if (url.pathname.startsWith(_serviceWorkerScope)) {
      event.respondWith(
        (async () => {
          try {
            await waitForHandshake(urlStr);
          } catch (e) {
            console.error((e as Error).message);
            return new Response((e as Error).message, {
              status: 503,
              statusText: "Service Unavailable",
            });
          }
          const relativePathToCwd =
            (_targetDirToCwd ?? ".") +
            url.pathname.replace(_serviceWorkerScope, "");
          return readFileFromProject(relativePathToCwd);
        })(),
      );
    }
  }
});

async function readFileFromProject(projectPath: string): Promise<Response> {
  try {
    const content = await _projectEndpoint.readFile(projectPath);

    let mimeType = "application/octet-stream";
    if (projectPath.endsWith(".js")) {
      mimeType = "application/javascript";
    } else if (projectPath.endsWith(".css")) {
      mimeType = "text/css";
    } else if (projectPath.endsWith(".html")) {
      mimeType = "text/html";
    } else if (projectPath.endsWith(".json")) {
      mimeType = "application/json";
    }

    return new Response(content, {
      headers: {
        "Content-Type": mimeType,
        ...(mimeType === "text/html"
          ? { "Cross-Origin-Embedder-Policy": "credentialless" }
          : {}),
      },
    });
  } catch (e) {
    console.error(`File ${projectPath} not found`);
    return new Response("Not Found", { status: 404 });
  }
}
