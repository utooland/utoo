// MessagePort-based HMR connection for browser environments (utoo-web)
// This replaces WebSocket when running in an iframe with HmrServer

type WebSocketMessage =
  | {
      type: "turbopack-connected";
    }
  | {
      type: "turbopack-message";
      data: Record<string, any>;
    };

let port: MessagePort | null = null;
let eventCallbacks: Array<(event: WebSocketMessage) => void> = [];

// Helper function to dispatch messages to all event callbacks
function dispatchMessage(message: WebSocketMessage) {
  for (const eventCallback of eventCallbacks) {
    eventCallback(message);
  }
}

export function addMessageListener(
  callback: (event: WebSocketMessage) => void,
) {
  eventCallbacks.push(callback);
}

export function sendMessage(data: any) {
  if (port) {
    const message = typeof data === "string" ? data : JSON.stringify(data);
    port.postMessage(message);
  }
}

export interface HMROptions {
  // Not used for MessagePort, kept for API compatibility
  path?: string;
}

let reloading = false;
let serverSessionId: number | null = null;

function handleMessage(event: MessageEvent<string>) {
  if (reloading) {
    return;
  }

  try {
    const msg =
      typeof event.data === "string" ? JSON.parse(event.data) : event.data;

    // Handle the different message formats from HmrServer
    if (msg.action === "turbopack-connected") {
      if (serverSessionId !== null && serverSessionId !== msg.data.sessionId) {
        window.location.reload();
        reloading = true;
        return;
      }

      serverSessionId = msg.data.sessionId;

      // Convert to turbopack format and trigger handleSocketConnected
      const connected: WebSocketMessage = { type: "turbopack-connected" };
      dispatchMessage(connected);
      return;
    }

    if (msg.action === "reload") {
      window.location.reload();
      reloading = true;
      return;
    }

    if (msg.action === "turbopack-message") {
      const turbopackMessage: WebSocketMessage = {
        type: "turbopack-message",
        data: msg.data,
      };
      dispatchMessage(turbopackMessage);
      return;
    }

    // Handle direct turbopack-dev-server messages
    if (
      msg.type &&
      ["partial", "restart", "notFound", "issues"].includes(msg.type)
    ) {
      const turbopackMessage: WebSocketMessage = {
        type: "turbopack-message",
        data: msg,
      };
      dispatchMessage(turbopackMessage);
      return;
    }

    // TODO: handle rest msg.actions
  } catch (e) {
    console.error("[HMR] Failed to parse message:", e);
  }
}

/**
 * Connect to HMR server via MessagePort.
 * The iframe sends "hmr-ready" to parent, and parent responds with "hmr-connect"
 * containing the MessagePort for bidirectional communication.
 */
export function connectHMR(_options?: HMROptions) {
  if (typeof window === "undefined") {
    return;
  }

  console.log("[HMR] waiting for MessagePort connection...");

  window.addEventListener("message", (event) => {
    // Check if this is an HMR connect message with a port
    if (event.data?.type === "hmr-connect" && event.ports.length > 0) {
      if (port) {
        // Already connected, close previous port
        port.close();
      }

      port = event.ports[0];
      port.onmessage = handleMessage;
      port.onmessageerror = () => {
        console.error("[HMR] MessagePort error");
        port = null;
      };

      console.log("[HMR] connected via MessagePort");

      // Don't dispatch connected event here - the server will send
      // a "turbopack-connected" message through the port which will
      // trigger the connected event via handleMessage
    }
  });

  // Signal to parent that HMR client is ready
  if (window.parent && window.parent !== window) {
    window.parent.postMessage({ type: "hmr-ready" }, "*");
    console.log("[HMR] sent hmr-ready to parent");
  }
}

/**
 * Check if HMR is connected
 */
export function isConnected(): boolean {
  return port !== null;
}

/**
 * Disconnect HMR
 */
export function disconnect() {
  if (port) {
    port.close();
    port = null;
  }
  eventCallbacks = [];
  serverSessionId = null;
  reloading = false;
}
