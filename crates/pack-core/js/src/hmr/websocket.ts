// Adapted from https://github.com/vercel/next.js/blob/canary/packages/next/src/client/dev/error-overlay/websocket.ts

type WebSocketMessage =
  | {
      type: "turbopack-connected";
    }
  | {
      type: "turbopack-message";
      data: Record<string, any>;
    };

let source: WebSocket | null = null;
let eventCallbacks: Array<(event: WebSocketMessage) => void> = [];

const RECONNECT_DELAYS_MS = [1_000, 2_000, 5_000, 10_000, 30_000];

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
  if (source && source.readyState === source.OPEN) {
    const message = typeof data === "string" ? data : JSON.stringify(data);
    source.send(message);
  }
}

function getSocketProtocol() {
  return typeof location !== "undefined" && location.protocol === "https:"
    ? "wss"
    : "ws";
}

function getSocketUrl() {
  const socketServer = process.env.SOCKET_SERVER;
  if (socketServer) {
    try {
      const parsed = new URL(socketServer);
      const protocol =
        parsed.protocol === "https:"
          ? "wss:"
          : parsed.protocol === "http:"
            ? "ws:"
            : parsed.protocol;

      if (protocol === "ws:" || protocol === "wss:") {
        const pathname =
          parsed.pathname === "/" ? "" : parsed.pathname.replace(/\/+$/, "");
        return `${protocol}//${parsed.host}${pathname}`;
      }
    } catch {}
  }

  const { hostname, port } = location;
  const protocol = getSocketProtocol();
  return `${protocol}://${hostname}${port ? `:${port}` : ""}`;
}

export interface HMROptions {
  path: string;
}

let reloading = false;
let serverSessionId: number | null = null;
let hasConnected = false;
let reconnectAttempt = 0;
let reconnectTimer: ReturnType<typeof setTimeout> | undefined;

// This is not used by Next.js, but it is used by the standalone turbopack-cli
export function connectHMR(options: HMROptions) {
  function init() {
    if (source) source.close();

    console.log("[HMR] connecting...");

    function handleOnline() {
      hasConnected = true;
      reconnectAttempt = 0;
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
        reconnectTimer = undefined;
      }
      window.console.log("[HMR] connected");

      // Send the turbopack-connected message to trigger handleSocketConnected
      const connected: WebSocketMessage = { type: "turbopack-connected" };
      dispatchMessage(connected);
    }

    function handleMessage(event: MessageEvent<string>) {
      if (reloading) {
        return;
      }

      try {
        const msg = JSON.parse(event.data);

        // Handle the different message formats from different servers
        if (msg.action === "turbopack-connected") {
          if (
            serverSessionId !== null &&
            serverSessionId !== msg.data.sessionId
          ) {
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

    function handleDisconnect(event?: Event) {
      if (event && event.target !== source) {
        return;
      }

      if (source) {
        source.onerror = null;
        source.onclose = null;
        source.close();
        source = null;
      }

      window.console.warn("[HMR] disconnected");

      // Do not retry a socket that never connected. Some proxy tools cannot
      // forward HMR and would otherwise receive repeated connection attempts.
      if (!hasConnected || reconnectTimer) {
        return;
      }

      const reconnectDelay = RECONNECT_DELAYS_MS[reconnectAttempt];
      if (reconnectDelay === undefined) {
        window.console.warn("[HMR] reconnect attempts exhausted");
        return;
      }

      reconnectAttempt++;
      reconnectTimer = setTimeout(() => {
        reconnectTimer = undefined;
        init();
      }, reconnectDelay);
    }

    source = new WebSocket(`${getSocketUrl()}${options.path}`);
    source.onopen = handleOnline;
    source.onerror = handleDisconnect;
    source.onclose = handleDisconnect;
    source.onmessage = handleMessage;
  }

  init();
}
