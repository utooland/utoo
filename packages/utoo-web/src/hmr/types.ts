/**
 * HMR (Hot Module Replacement) types for browser-based utoopack.
 * These types are adapted from pack-core to work with MessagePort communication.
 */

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

// Turbopack update types
export interface EcmascriptMergedUpdate {
  type: "EcmascriptMergedUpdate";
  chunks: { [moduleName: string]: { type: "partial" } };
  entries: { [moduleName: string]: { code: string; map: string; url: string } };
}

export interface BaseUpdate {
  resource: ResourceIdentifier;
  diagnostics: unknown[];
  issues: HmrIssue[];
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

export type TurbopackUpdate = IssuesUpdate | PartialUpdate;

// HMR action types sent to browser
export const enum HMR_ACTIONS_SENT_TO_BROWSER {
  RELOAD = "reload",
  SYNC = "sync",
  BUILT = "built",
  BUILDING = "building",
  TURBOPACK_MESSAGE = "turbopack-message",
  TURBOPACK_CONNECTED = "turbopack-connected",
}

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

// Client to server message types
export interface TurbopackSubscribeMessage {
  type: "turbopack-subscribe";
  path: string;
}

export interface TurbopackUnsubscribeMessage {
  type: "turbopack-unsubscribe";
  path: string;
}

export type HmrClientMessage =
  | TurbopackSubscribeMessage
  | TurbopackUnsubscribeMessage
  | { event: string; [key: string]: unknown };

// Update info subscription types
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
