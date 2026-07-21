/**
 * Shared HMR (Hot Module Replacement) types for utoopack.
 * Used by both NAPI (Node.js) and browser-based implementations.
 */

// ============================================================================
// Resource and Issue Types
// ============================================================================

export interface ResourceIdentifier {
  path: string;
  headers?: unknown;
}

export interface HmrIssue {
  severity: string;
  stage: string;
  filePath: string;
  title: unknown;
  description?: unknown;
  detail?: unknown;
  source?: HmrIssueSource;
  documentationLink: string;
  importTraces: unknown;
}

export interface HmrIssueSource {
  source: {
    ident: string;
    content?: string;
  };
  range?: {
    start: { line: number; column: number };
    end: { line: number; column: number };
  };
}

// ============================================================================
// Turbopack Update Types
// ============================================================================

export interface EcmascriptMergedUpdate {
  type: "EcmascriptMergedUpdate";
  chunks: { [moduleName: string]: { type: "partial" } };
  entries: { [moduleName: string]: { code: string; map: string; url: string } };
}

export interface BaseUpdate {
  resource: ResourceIdentifier;
  issues: HmrIssue[];
  validation?: string;
}

export interface IssuesUpdate extends BaseUpdate {
  type: "issues";
}

export interface PartialUpdate extends BaseUpdate {
  type: "partial";
  instruction: {
    type: "ChunkListUpdate";
    merged: EcmascriptMergedUpdate[] | undefined;
  };
}

export interface NotFoundUpdate {
  type: "notFound";
  resource: ResourceIdentifier;
  issues: HmrIssue[];
}

export interface RestartUpdate {
  type: "restart";
  resource: ResourceIdentifier;
  issues: HmrIssue[];
}

export type TurbopackUpdate =
  | IssuesUpdate
  | PartialUpdate
  | NotFoundUpdate
  | RestartUpdate;

// ============================================================================
// HMR Actions (Server -> Client)
// ============================================================================

export enum HMR_ACTIONS_SENT_TO_BROWSER {
  RELOAD = "reload",
  CLIENT_CHANGES = "clientChanges",
  SERVER_ONLY_CHANGES = "serverOnlyChanges",
  SYNC = "sync",
  BUILT = "built",
  BUILDING = "building",
  TURBOPACK_MESSAGE = "turbopack-message",
  TURBOPACK_CONNECTED = "turbopack-connected",
}

export type HmrActionType = HMR_ACTIONS_SENT_TO_BROWSER;

export interface TurbopackMessageAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_MESSAGE;
  data: TurbopackUpdate | TurbopackUpdate[];
}

export interface TurbopackConnectedAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.TURBOPACK_CONNECTED;
  data: { sessionId: number };
}

export interface BuildingAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.BUILDING;
}

export interface CompilationError {
  moduleName?: string;
  message: string;
  details?: string;
  moduleTrace?: Array<{ moduleName?: string }>;
  stack?: string;
}

export interface SyncAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.SYNC;
  hash: string;
  errors: ReadonlyArray<CompilationError>;
  warnings: ReadonlyArray<CompilationError>;
  updatedModules?: ReadonlyArray<string>;
}

export interface BuiltAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.BUILT;
  hash: string;
  errors: ReadonlyArray<CompilationError>;
  warnings: ReadonlyArray<CompilationError>;
  updatedModules?: ReadonlyArray<string>;
}

export interface ReloadAction {
  action: HMR_ACTIONS_SENT_TO_BROWSER.RELOAD;
  data: string;
}

export type HMR_ACTION_TYPES =
  | TurbopackMessageAction
  | TurbopackConnectedAction
  | BuildingAction
  | SyncAction
  | BuiltAction
  | ReloadAction;

// ============================================================================
// HMR Messages (Client -> Server)
// ============================================================================

export interface TurbopackSubscribeMessage {
  type: "turbopack-subscribe";
  path: string;
  version?: string;
  validation?: string;
}

export interface TurbopackUnsubscribeMessage {
  type: "turbopack-unsubscribe";
  path: string;
}

export type HmrClientMessage =
  | TurbopackSubscribeMessage
  | TurbopackUnsubscribeMessage
  | { event: string; [key: string]: unknown };

// ============================================================================
// Update Info Types
// ============================================================================

export interface UpdateInfo {
  duration: number;
  tasks: number;
}

export interface UpdateStartMessage {
  updateType: "start";
}

export interface UpdateEndMessage {
  updateType: "end";
  value: UpdateInfo;
}

export type UpdateMessage = UpdateStartMessage | UpdateEndMessage;
