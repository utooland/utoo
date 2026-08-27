/**
 * HmrServer - Simulates a WebSocket server using MessagePort for HMR communication.
 *
 * This class manages HMR connections with preview iframes, forwarding Turbopack
 * HMR events from the main project to the preview iframe's HMR client.
 *
 * Architecture:
 * - Main Thread (Project) -> HmrServer -> MessagePort -> Preview Iframe (HMR Client)
 */

import type {
  HMR_ACTION_TYPES,
  HmrClientMessage,
  TurbopackUpdate,
} from "@utoo/pack-shared";
import { HMR_ACTIONS_SENT_TO_BROWSER } from "@utoo/pack-shared";

export interface HmrServerOptions {
  /** Session ID for this HMR server instance */
  sessionId?: number;
  /** Callback when a client subscribes to an HMR path */
  onSubscribe?: (path: string, client: HmrClient) => void;
  /** Callback when a client unsubscribes from an HMR path */
  onUnsubscribe?: (path: string, client: HmrClient) => void;
}

export interface HmrClient {
  /** Unique client ID */
  id: string;
  /** The MessagePort for this client */
  port: MessagePort;
  /** Send a message to this client */
  send(message: HMR_ACTION_TYPES): void;
  /** Close the connection to this client */
  close(): void;
}

let clientIdCounter = 0;

/**
 * HmrServer manages HMR connections with preview iframes.
 */
export class HmrServer {
  private clients = new Set<HmrClient>();
  private subscriptions = new Map<string, Set<HmrClient>>();
  private sessionId: number;
  private options: HmrServerOptions;
  private hmrHash = 0;

  constructor(options: HmrServerOptions = {}) {
    this.options = options;
    this.sessionId =
      options.sessionId ?? Math.floor(Number.MAX_SAFE_INTEGER * Math.random());
  }

  /**
   * Create a MessageChannel and return the port that should be sent to the iframe.
   * The other port is kept by the server for communication.
   */
  public createConnection(): { clientPort: MessagePort; client: HmrClient } {
    const { port1, port2 } = new MessageChannel();
    const clientId = `hmr-client-${++clientIdCounter}`;

    const client: HmrClient = {
      id: clientId,
      port: port1,
      send: (message: HMR_ACTION_TYPES) => {
        port1.postMessage(JSON.stringify(message));
      },
      close: () => {
        this.removeClient(client);
        port1.close();
      },
    };

    port1.onmessage = (event) => {
      this.handleClientMessage(client, event.data);
    };

    port1.onmessageerror = () => {
      this.removeClient(client);
    };

    this.clients.add(client);

    // Send connected message
    client.send({
      action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_CONNECTED,
      data: { sessionId: this.sessionId },
    });

    // Send initial sync
    client.send({
      action: HMR_ACTIONS_SENT_TO_BROWSER.SYNC,
      hash: String(this.hmrHash),
      errors: [],
      warnings: [],
    });

    return { clientPort: port2, client };
  }

  /**
   * Connect an iframe to the HMR server.
   * This posts a message to the iframe with the client port.
   */
  public connectIframe(
    iframe: HTMLIFrameElement,
    origin: string = "*",
  ): HmrClient | null {
    if (!iframe.contentWindow) {
      console.warn("[HmrServer] Cannot connect: iframe has no contentWindow");
      return null;
    }

    const { clientPort, client } = this.createConnection();

    // Send the port to the iframe
    iframe.contentWindow.postMessage(
      { type: "hmr-connect", sessionId: this.sessionId },
      origin,
      [clientPort],
    );

    return client;
  }

  private handleClientMessage(client: HmrClient, data: string) {
    try {
      const message: HmrClientMessage =
        typeof data === "string" ? JSON.parse(data) : data;

      if ("type" in message) {
        switch (message.type) {
          case "turbopack-subscribe":
            this.subscribe(client, (message as { path: string }).path);
            break;
          case "turbopack-unsubscribe":
            this.unsubscribe(client, (message as { path: string }).path);
            break;
        }
      }

      // Handle other client events (client-error, client-success, etc.)
      if ("event" in message) {
        this.handleClientEvent(client, message);
      }
    } catch (e) {
      console.error("[HmrServer] Failed to parse client message:", e);
    }
  }

  private handleClientEvent(
    _client: HmrClient,
    message: { event: string; hadRuntimeError?: unknown },
  ) {
    switch (message.event) {
      case "client-error":
      case "client-warning":
      case "client-success":
        // These are informational, can be logged or forwarded
        break;
      case "client-full-reload":
        if (message.hadRuntimeError) {
          console.warn(
            "[HmrServer] Fast Refresh had to perform a full reload due to a runtime error.",
          );
        }
        break;
    }
  }

  private subscribe(client: HmrClient, path: string) {
    let clients = this.subscriptions.get(path);
    if (!clients) {
      clients = new Set();
      this.subscriptions.set(path, clients);
    }
    clients.add(client);
    this.options.onSubscribe?.(path, client);
  }

  private unsubscribe(client: HmrClient, path: string) {
    const clients = this.subscriptions.get(path);
    if (clients) {
      clients.delete(client);
      if (clients.size === 0) {
        this.subscriptions.delete(path);
      }
    }
    this.options.onUnsubscribe?.(path, client);
  }

  private removeClient(client: HmrClient) {
    this.clients.delete(client);
    // Remove from all subscriptions
    for (const [path, clients] of this.subscriptions) {
      if (clients.has(client)) {
        clients.delete(client);
        this.options.onUnsubscribe?.(path, client);
      }
      if (clients.size === 0) {
        this.subscriptions.delete(path);
      }
    }
  }

  /**
   * Send a Turbopack update to all subscribed clients for a specific path.
   */
  public sendUpdate(path: string, update: TurbopackUpdate) {
    const clients = this.subscriptions.get(path);
    if (!clients) return;

    const message: HMR_ACTION_TYPES = {
      action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_MESSAGE,
      data: update,
    };

    for (const client of clients) {
      client.send(message);
    }
  }

  /**
   * Broadcast an HMR message to all connected clients.
   */
  public broadcast(message: HMR_ACTION_TYPES) {
    for (const client of this.clients) {
      client.send(message);
    }
  }

  /**
   * Send building notification to all clients.
   */
  public sendBuilding() {
    this.broadcast({
      action: HMR_ACTIONS_SENT_TO_BROWSER.BUILDING,
    });
  }

  /**
   * Send built notification to all clients.
   */
  public sendBuilt(
    errors: readonly unknown[] = [],
    warnings: readonly unknown[] = [],
  ) {
    this.hmrHash++;
    this.broadcast({
      action: HMR_ACTIONS_SENT_TO_BROWSER.BUILT,
      hash: String(this.hmrHash),
      errors: errors as any,
      warnings: warnings as any,
    });
  }

  /**
   * Send reload request to all clients.
   */
  public sendReload(reason: string) {
    this.broadcast({
      action: HMR_ACTIONS_SENT_TO_BROWSER.RELOAD,
      data: reason,
    });
  }

  /**
   * Get all currently subscribed paths.
   */
  public getSubscribedPaths(): string[] {
    return Array.from(this.subscriptions.keys());
  }

  /**
   * Get number of connected clients.
   */
  public get clientCount(): number {
    return this.clients.size;
  }

  /**
   * Close all client connections.
   */
  public close() {
    for (const client of this.clients) {
      client.close();
    }
    this.clients.clear();
    this.subscriptions.clear();
  }
}
