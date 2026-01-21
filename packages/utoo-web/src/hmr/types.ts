/**
 * HMR (Hot Module Replacement) types for browser-based utoopack.
 * Re-exports shared types from @utoo/pack-shared for convenience.
 */

// Re-export all shared HMR types
export {
  type BaseUpdate,
  type BuildingAction,
  type BuiltAction,
  type CompilationError,
  // Update Types
  type EcmascriptMergedUpdate,
  type HMR_ACTION_TYPES,
  // HMR Actions
  HMR_ACTIONS_SENT_TO_BROWSER,
  type HmrActionType,
  type HmrClientMessage,
  type HmrIssue,
  type HmrIssueSource,
  type IssuesUpdate,
  type NotFoundUpdate,
  type PartialUpdate,
  type ReloadAction,
  // Resource and Issue Types
  type ResourceIdentifier,
  type RestartUpdate,
  type SyncAction,
  type TurbopackConnectedAction,
  type TurbopackMessageAction,
  // Client Messages
  type TurbopackSubscribeMessage,
  type TurbopackUnsubscribeMessage,
  type TurbopackUpdate,
  type UpdateEndMessage,
  // Update Info
  type UpdateInfo,
  type UpdateMessage,
  type UpdateStartMessage,
} from "@utoo/pack-shared";
