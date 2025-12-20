import { Project } from "./project/Project";
import { ProjectEndpoint } from "./types";
import { ServiceWorkerHandShake } from "./utils/message";

declare let self: ServiceWorkerGlobalScope;

let _resolve: () => void;

let _promise: Promise<void> = new Promise((resolve) => {
  _resolve = resolve;
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
  if (event.data && event.data[ServiceWorkerHandShake] === true) {
    _projectEndpoint = Project.fork(
      new MessageChannel(),
      event.source as Client,
    );
    _resolve();
  }
});

self.addEventListener("fetch", (event: FetchEvent) => {
  let { url: urlStr } = event.request;
  let url = new URL(urlStr);
  if (typeof _serviceWorkerScope === "string") {
    if (url.pathname.startsWith(_serviceWorkerScope)) {
      event.respondWith(
        (async () => {
          await _promise;
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
