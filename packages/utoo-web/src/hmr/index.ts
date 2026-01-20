/**
 * HMR (Hot Module Replacement) module for browser-based utoopack.
 *
 * This module provides HMR support for preview iframes using MessagePort
 * instead of WebSocket for communication.
 *
 * Usage:
 *
 * In the main application:
 * ```typescript
 * import { HmrServer } from "@utoo/web/hmr";
 *
 * const hmrServer = new HmrServer();
 * const client = hmrServer.connectIframe(iframeElement);
 *
 * // When you receive HMR updates from the build system:
 * hmrServer.sendUpdate(path, update);
 * ```
 *
 * The HMR client (bootstrap, client) is in crates/pack-core/js/src/hmr/
 * and is injected into the preview iframe by the build system.
 */

export { type HmrClient, HmrServer, type HmrServerOptions } from "./HmrServer";
export * from "./types";
