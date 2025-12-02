import * as comlink from "comlink";
import { internalEndpoint } from "./internalProject";
import { HandShake } from "./message";

declare let self: DedicatedWorkerGlobalScope;

const ConnectedPorts = new Set<MessagePort>();

self.addEventListener("message", (e) => {
  const port = e.ports[0];
  if (e.data === HandShake && !ConnectedPorts.has(port)) {
    comlink.expose(internalEndpoint, port);
    ConnectedPorts.add(port);
  }
});
