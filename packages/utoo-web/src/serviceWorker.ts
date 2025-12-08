import { Project, ProjectEndpoint } from ".";
import { ServiceWorkerHandShake } from "./message";

declare let self: ServiceWorkerGlobalScope;

let _resolve: () => void;

let _promise: Promise<void> = new Promise((resolve) => {
  _resolve = resolve;
});

let _projectEndpoint: ProjectEndpoint;

let _serviceWorkerScope: string;
let _relativeDirToCwd: string | undefined;

self.addEventListener("install", (event) => {
  event.waitUntil(self.skipWaiting());
});

self.addEventListener("activate", (event) => {
  event.waitUntil(self.clients.claim());
});

self.addEventListener("message", (event) => {
  if (event.data && event.data[ServiceWorkerHandShake] === true) {
    _serviceWorkerScope = event.data.scope;
    _relativeDirToCwd = event.data.relativeDirToCwd;
    _projectEndpoint = Project.fork(
      new MessageChannel(),
      event.source as Client,
    );
    _resolve();
  }
});

self.addEventListener("fetch", async (event: FetchEvent) => {
  await _promise;
  let { url: url_str, referrer } = event.request;
  let url = new URL(url_str);
  if (
    url.pathname.startsWith(_serviceWorkerScope) ||
    (referrer && new URL(referrer).pathname.startsWith(_serviceWorkerScope))
  ) {
    const relateivePathToCwd =
      (_relativeDirToCwd ?? ".") +
      url.pathname.replace(_serviceWorkerScope, "");
    event.respondWith(readFileFromProject(relateivePathToCwd));
  } else {
    return;
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
          ? { "Cross-Origin-Embedder-Policy": "require-corp" }
          : {}),
      },
    });
  } catch (e) {
    return new Response("Not Found", { status: 404 });
  }
}
