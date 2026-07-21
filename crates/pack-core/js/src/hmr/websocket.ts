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

const INITIAL_RECONNECT_DELAY_MS = 500;
const MAX_RECONNECT_DELAY_MS = 5_000;

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

// This is not used by Next.js, but it is used by the standalone turbopack-cli
export function connectHMR(options: HMROptions) {
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let reconnectAttempts = 0;
  let pageIsUnloading = false;

  function clearReconnectTimer() {
    if (reconnectTimer !== null) {
      clearTimeout(reconnectTimer);
      reconnectTimer = null;
    }
  }

  function closeSocket(socket: WebSocket) {
    socket.onopen = null;
    socket.onerror = null;
    socket.onclose = null;
    socket.onmessage = null;

    if (source === socket) {
      source = null;
    }

    if (
      socket.readyState === WebSocket.CONNECTING ||
      socket.readyState === WebSocket.OPEN
    ) {
      socket.close();
    }
  }

  function scheduleReconnect() {
    if (reconnectTimer !== null || pageIsUnloading || reloading) {
      return;
    }

    const delay = Math.min(
      INITIAL_RECONNECT_DELAY_MS * 2 ** reconnectAttempts,
      MAX_RECONNECT_DELAY_MS,
    );
    reconnectAttempts += 1;
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      init();
    }, delay);
  }

  function init() {
    if (pageIsUnloading || reloading) {
      return;
    }

    if (source) {
      closeSocket(source);
    }

    console.log("[HMR] connecting...");

    let socket: WebSocket;
    try {
      socket = new WebSocket(`${getSocketUrl()}${options.path}`);
    } catch (error) {
      console.error("[HMR] Failed to create WebSocket:", error);
      scheduleReconnect();
      return;
    }

    source = socket;

    function handleOnline() {
      if (source !== socket || pageIsUnloading || reloading) {
        closeSocket(socket);
        return;
      }

      reconnectAttempts = 0;
      window.console.log("[HMR] connected");
      // Direct turbopack-dev-server does not send a separate connected frame.
      // Utoo does, but the socket-open notification is sufficient to restore
      // subscriptions in both cases.
      dispatchMessage({ type: "turbopack-connected" });
    }

    function handleMessage(event: MessageEvent<string>) {
      if (source !== socket || reloading) {
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
            reloading = true;
            window.location.reload();
            return;
          }

          serverSessionId = msg.data.sessionId;

          // Socket open already restored subscriptions. This frame only carries
          // the Utoo server session id used to detect server restarts.
          return;
        }

        if (msg.action === "reload") {
          reloading = true;
          window.location.reload();
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

    function handleDisconnect(event: Event) {
      if (event.target !== socket || source !== socket) {
        return;
      }

      closeSocket(socket);
      window.console.warn("[HMR] disconnected");
      scheduleReconnect();
    }

    socket.onopen = handleOnline;
    socket.onerror = handleDisconnect;
    socket.onclose = handleDisconnect;
    socket.onmessage = handleMessage;
  }

  // `pagehide` is not fired when a beforeunload prompt is cancelled, so it is
  // safe to use as the point where reconnects should stop.
  window.addEventListener("pagehide", () => {
    pageIsUnloading = true;
    clearReconnectTimer();
    if (source) {
      closeSocket(source);
    }
  });

  // A page restored from the back-forward cache needs a fresh HMR transport.
  window.addEventListener("pageshow", (event) => {
    if (event.persisted) {
      pageIsUnloading = false;
      reconnectAttempts = 0;
      init();
    }
  });

  init();
}
