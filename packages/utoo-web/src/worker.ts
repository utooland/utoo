import * as comlink from "comlink";
import { internalEndpoint } from "./project/InternalProject";
import { HandShake } from "./utils/message";

declare let self: DedicatedWorkerGlobalScope;

const ConnectedPorts = new Set<MessagePort>();

self.addEventListener("message", (e) => {
  const port = e.ports[0];
  if (e.data === HandShake && !ConnectedPorts.has(port)) {
    comlink.expose(internalEndpoint, port);
    ConnectedPorts.add(port);
  }
});
