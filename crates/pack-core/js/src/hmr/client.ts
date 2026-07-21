// @ts-ignore
import { connect } from "@vercel/turbopack-ecmascript-runtime/browser/dev/hmr-client/hmr-client";
import { addMessageListener, connectHMR, sendMessage } from "./websocket";

export function initHMR(chunkUpdateListenersGlobal: string) {
  connect({
    addMessageListener,
    sendMessage,
    onUpdateError: console.error,
    chunkUpdateListenersGlobal,
  });
  connectHMR({
    path: "/turbopack-hmr",
  });
}
