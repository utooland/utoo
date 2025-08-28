import { Project, ProjectEndpoint } from ".";
import { ServiceWorkerHandShake } from "./message";

declare let self: ServiceWorkerGlobalScope;

let __resolve__: () => void;

let _promise__: Promise<void> = new Promise((resolve) => {
  __resolve__ = resolve;
});

let projectEndpoint: ProjectEndpoint | undefined;

let proxiedResourcePath: string | undefined;

self.addEventListener("message", (event) => {
  if (event.data && event.data[ServiceWorkerHandShake] === true) {
    proxiedResourcePath = event.data.proxiedResourcePath;
    projectEndpoint = Project.fork(
      new MessageChannel(),
      event.source as Client,
    );
    __resolve__();
  }
});

self.addEventListener("fetch", async (event: FetchEvent) => {
  const { url, referrer } = event.request;
  if (
    new URL(url).pathname.startsWith(`/${proxiedResourcePath}`) ||
    (referrer &&
      new URL(referrer).pathname.startsWith(`/${proxiedResourcePath}`))
  ) {
    await _promise__;
    const projectPath =
      "." + new URL(url).pathname.replace(`/${proxiedResourcePath}`, "");
    event.respondWith(readFileFromProject(projectPath));
  } else {
    event.respondWith(fetch(event.request));
  }
});

async function readFileFromProject(projectPath: string): Promise<Response> {
  try {
    const content = await projectEndpoint!.readFile(projectPath);

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
