// MessagePort-based HMR client for utoo-web (browser/wasm environment)
// This file is injected instead of client.ts when building for wasm target

// @ts-ignore
import { connect } from "@vercel/turbopack-ecmascript-runtime/browser/dev/hmr-client/hmr-client";
import { addMessageListener, connectHMR, sendMessage } from "./messageport";

export function initHMR(chunkUpdateListenersGlobal: string) {
  // First register the HMR client listeners
  connect({
    addMessageListener,
    sendMessage,
    onUpdateError: console.error,
    chunkUpdateListenersGlobal,
  });

  // Then start listening for MessagePort connection from parent window
  // The actual "turbopack-connected" event will be dispatched when
  // the parent window sends the "hmr-connect" message with the port
  connectHMR();
}
