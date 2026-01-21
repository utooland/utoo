/**
 * HMR (Hot Module Replacement) module for browser-based utoopack.
 *
 * This module provides HMR support for preview iframes using MessagePort
 * instead of WebSocket for communication.
 *
 * Usage:
 *
 * ```typescript
 * import { Project } from "@utoo/web";
 *
 * const project = new Project({ ... });
 *
 * // Start dev mode (creates HmrServer internally)
 * project.dev((result) => {
 *   console.log("Build complete", result);
 * });
 *
 * // Connect iframe to receive HMR updates
 * const client = project.connectHmrIframe(iframeElement);
 * ```
 *
 * The HMR client runtime is in crates/pack-core/js/src/hmr/
 * and is injected into the preview iframe by the build system.
 */

export {
  type BaseUpdate,
  type BuildingAction,
  type BuiltAction,
  type CompilationError,
  type EcmascriptMergedUpdate,
  type HMR_ACTION_TYPES,
  HMR_ACTIONS_SENT_TO_BROWSER,
  type HmrActionType,
  type HmrClientMessage,
  type HmrIssue,
  type HmrIssueSource,
  type IssuesUpdate,
  type NotFoundUpdate,
  type PartialUpdate,
  type ReloadAction,
  type ResourceIdentifier,
  type RestartUpdate,
  type SyncAction,
  type TurbopackConnectedAction,
  type TurbopackMessageAction,
  type TurbopackSubscribeMessage,
  type TurbopackUnsubscribeMessage,
  type TurbopackUpdate,
  type UpdateEndMessage,
  type UpdateInfo,
  type UpdateMessage,
  type UpdateStartMessage,
} from "@utoo/pack-shared";
export { type HmrClient, HmrServer, type HmrServerOptions } from "./HmrServer";
