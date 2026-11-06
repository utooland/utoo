import {
  HmrIdentifiers,
  NapiIssue,
  NapiUpdateMessage,
  NapiWrittenEndpoint,
  StackFrame,
} from "../binding";
import { BundleOptions } from "../config/types";

declare global {
  export type TurbopackResult<T = {}> = T & {
    issues: NapiIssue[];
  };
  export type RefCell = { readonly __tag: unique symbol };
  export type ExternalEndpoint = { readonly __tag: unique symbol };
  export type RcStr = string;
}

export interface BaseUpdate {
  resource: {
    headers: unknown;
    path: string;
  };
  issues: NapiIssue[];
}

export interface IssuesUpdate extends BaseUpdate {
  type: "issues";
}

export interface EcmascriptMergedUpdate {
  type: "EcmascriptMergedUpdate";
  chunks: { [moduleName: string]: { type: "partial" } };
  entries: { [moduleName: string]: { code: string; map: string; url: string } };
}

export interface PartialUpdate extends BaseUpdate {
  type: "partial";
  instruction: {
    type: "ChunkListUpdate";
    merged: EcmascriptMergedUpdate[] | undefined;
  };
}

export type Update = IssuesUpdate | PartialUpdate;

export interface ProjectOptions extends BundleOptions {
  /**
   * A root path from which all files must be nested under. Trying to access
   * a file outside this root will fail. Think of this as a chroot.
   * Defaults to automatic root detection from projectPath. UTOO_ROOT_PATH
   * overrides both explicit paths and automatic detection.
   */
  rootPath: string;

  /**
   * A path inside the root_path which contains the app/pages directories.
   */
  projectPath: string;
}

export interface Project {
  update(options: Partial<ProjectOptions>): Promise<void>;

  writeAllEntrypointsToDisk(): Promise<TurbopackResult<RawEntrypoints>>;

  /** Writes native analysis data for browser entries, without emitting bundles. */
  writeAnalyzeData(outputPath: string): Promise<TurbopackResult<AnalyzeData>>;

  /**
   * Activates the lazily compiled dynamic import whose manifest chunk is
   * `chunkPath` (relative to the output root). Returns whether the path named
   * one; the owning entrypoints must be rewritten before the chunk is served.
   */
  activateLazyChunk(chunkPath: string): Promise<boolean>;

  entrypointsSubscribe(): AsyncIterableIterator<
    TurbopackResult<RawEntrypoints>
  >;

  hmrEvents(
    identifier: string,
    expectedVersion?: string,
  ): AsyncIterableIterator<TurbopackResult<Update>>;

  hmrIdentifiersSubscribe(): AsyncIterableIterator<
    TurbopackResult<HmrIdentifiers>
  >;

  getSourceForAsset(filePath: string): Promise<string | null>;

  getSourceMap(filePath: string): Promise<string | null>;
  getSourceMapSync(filePath: string): string | null;

  traceSource(
    stackFrame: StackFrame,
    currentDirectoryFileUrl: string,
  ): Promise<StackFrame | null>;

  getCompletedTaskCount(): number;

  updateInfoSubscribe(
    aggregationMs: number,
  ): AsyncIterableIterator<NapiUpdateMessage>;

  shutdown(): Promise<void>;

  onExit(): Promise<void>;
}

export interface AnalyzeEntry {
  /** The configured Pack entry name. */
  name: string;
  /** Report-relative entry key, independent of user-provided names. */
  path: string;
  /** The runtime that owns this entry's output. */
  environment: "client" | "server";
}

export interface AnalyzeData {
  entries: AnalyzeEntry[];
}

export interface RawEntrypoints {
  apps?: Endpoint[];
  libraries?: Endpoint[];
  appPaths?: NapiWrittenEndpoint[];
  libraryPaths?: NapiWrittenEndpoint[];
}

export interface Endpoint {
  /** Write files for the endpoint to disk. */
  writeToDisk(): Promise<TurbopackResult<NapiWrittenEndpoint>>;

  /**
   * Listen to client-side changes to the endpoint.
   * After clientChanged() has been awaited it will listen to changes.
   * The async iterator will yield for each change.
   */
  clientChanged(): Promise<AsyncIterableIterator<TurbopackResult>>;

  /**
   * Listen to server-side changes to the endpoint.
   * After serverChanged() has been awaited it will listen to changes.
   * The async iterator will yield for each change.
   */
  serverChanged(
    includeIssues: boolean,
  ): Promise<AsyncIterableIterator<TurbopackResult>>;
}
