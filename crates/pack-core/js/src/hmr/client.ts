// @ts-ignore
import { connect } from "@vercel/turbopack-ecmascript-runtime/browser/dev/hmr-client/hmr-client";
import { addMessageListener, connectHMR, sendMessage } from "./websocket";

export function initHMR() {
  connect({
    addMessageListener,
    sendMessage,
    onUpdateError: console.error,
  });
  connectHMR({
    path: "/turbopack-hmr",
  });
}
