import { Project, ProjectEndpoint } from ".";
import { ServiceWorkerHandShake } from "./message";

declare let self: ServiceWorkerGlobalScope;

let _resolve: () => void;

let _promise: Promise<void> = new Promise((resolve) => {
  _resolve = resolve;
});

let _projectEndpoint: ProjectEndpoint;

let _serviceWorkerScope: string;
let _targetDirToCwd: string | undefined;

self.addEventListener("install", (event) => {
  event.waitUntil(self.skipWaiting());
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("message", (event) => {
  if (event.data && event.data[ServiceWorkerHandShake] === true) {
    _serviceWorkerScope = event.data.scope;
    _targetDirToCwd = event.data.targetDirToCwd;
    _projectEndpoint = Project.fork(
      new MessageChannel(),
      event.source as Client,
    );
    _resolve();
  }
});

self.addEventListener("fetch", (event: FetchEvent) => {
  event.respondWith(
    (async () => {
      await _promise;
      let { url: url_str, referrer } = event.request;
      let url = new URL(url_str);
      if (
        url.pathname.startsWith(_serviceWorkerScope) ||
        (referrer && new URL(referrer).pathname.startsWith(_serviceWorkerScope))
      ) {
        const relativePathToCwd =
          (_targetDirToCwd ?? ".") +
          url.pathname.replace(_serviceWorkerScope, "");
        return readFileFromProject(relativePathToCwd);
      } else {
        return fetch(event.request);
      }
    })(),
  );
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
          ? { "Cross-Origin-Embedder-Policy": "require-corp" }
          : {}),
      },
    });
  } catch (e) {
    return new Response("Not Found", { status: 404 });
  }
}
