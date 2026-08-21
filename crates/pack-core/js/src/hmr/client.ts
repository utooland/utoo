// @ts-ignore
import { connect } from "@vercel/turbopack-ecmascript-runtime/browser/dev/hmr-client/hmr-client";
import {
  addMessageListener,
  connectHMR,
  type HMRReconnect,
  sendMessage,
} from "./websocket";

export function initHMR(
  chunkUpdateListenersGlobal = "TURBOPACK_CHUNK_UPDATE_LISTENERS",
  reconnect: HMRReconnect = false,
) {
  connect({
    addMessageListener,
    sendMessage,
    onUpdateError: console.error,
    chunkUpdateListenersGlobal,
  });
  connectHMR({
    path: "/turbopack-hmr",
    reconnect,
  });
}
