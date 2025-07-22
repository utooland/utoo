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

export function addMessageListener(
  callback: (event: WebSocketMessage) => void,
) {
  eventCallbacks.push(callback);
}

export function sendMessage(data: any) {
  if (source && source.readyState === source.OPEN) {
    const message = typeof data === 'string' ? data : JSON.stringify(data);
    source.send(message);
  }
}

function getSocketProtocol() {
  return typeof location !== "undefined" && location.protocol === "https:"
    ? "wss"
    : "ws";
}

export interface HMROptions {
  path: string;
}

let reconnections = 0;
let reloading = false;
let serverSessionId: number | null = null;

// This is not used by Next.js, but it is used by the standalone turbopack-cli
export function connectHMR(options: HMROptions) {
  function init() {
    if (source) source.close();

    console.log("[HMR] connecting...");

    function handleOnline() {
      reconnections = 0;
      window.console.log("[HMR] connected");
      
      // Send the turbopack-connected message to trigger handleSocketConnected
      const connected: WebSocketMessage = { type: 'turbopack-connected' };
      for (const eventCallback of eventCallbacks) {
        eventCallback(connected);
      }
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
          const connected: WebSocketMessage = { type: 'turbopack-connected' };
          for (const eventCallback of eventCallbacks) {
            eventCallback(connected);
          }
          return;
        }

        if (msg.action === "reload") {
          window.location.reload();
          reloading = true;
          return;
        }

        if (msg.action === "turbopack-message") {
          const turbopackMessage: WebSocketMessage = { 
            type: 'turbopack-message', 
            data: msg.data 
          };
          for (const eventCallback of eventCallbacks) {
            eventCallback(turbopackMessage);
          }
          return;
        }

        // Handle direct turbopack-dev-server messages
        if (msg.type && ["partial", "restart", "notFound", "issues"].includes(msg.type)) {
          const turbopackMessage: WebSocketMessage = { 
            type: 'turbopack-message', 
            data: msg 
          };
          for (const eventCallback of eventCallbacks) {
            eventCallback(turbopackMessage);
          }
          return;
        }

        // TODO: handle rest msg.actions
      } catch (e) {
        console.error("[HMR] Failed to parse message:", e);
      }
    }

    let timer: ReturnType<typeof setTimeout>;
    function handleDisconnect() {
      source.onerror = null;
      source.onclose = null;
      source.close();
      reconnections++;
      // After 25 reconnects we'll want to reload the page as it indicates the dev server is no longer running.
      if (reconnections > 25) {
        reloading = true;
        window.location.reload();
        return;
      }

      clearTimeout(timer);
      // Try again after 5 seconds
      timer = setTimeout(init, reconnections > 5 ? 5000 : 1000);
    }

    const { hostname, port } = location;
    const protocol = getSocketProtocol();

    let url = `${protocol}://${hostname}:${port}`;

    source = new window.WebSocket(`${url}${options.path}`);
    source.onopen = handleOnline;
    source.onerror = handleDisconnect;
    source.onmessage = handleMessage;
  }

  init();
}
