/**
 * This file contains runtime types and functions that are shared between all
 * Turbopack UMD library runtimes (DOM and Node.js).
 *
 * It will be appended to the runtime code of each runtime right after the
 * shared runtime utils.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

/// <reference path="../../../../../next.js/turbopack/crates/turbopack-ecmascript-runtime/js/src/shared/runtime/runtime-utils.ts" />
/// <reference path="../../../../../next.js/turbopack/crates/turbopack-ecmascript-runtime/js/src/shared/runtime/runtime-types.d.ts" />

interface TurbopackBrowserBaseContext<M> extends TurbopackBaseContext<M> {
  R: ResolvePathFromModule;
}

const browserContextPrototype =
  Context.prototype as TurbopackBrowserBaseContext<unknown>;

// Provided by build
declare function instantiateModule(
  id: ModuleId,
  sourceType: SourceType,
  sourceData: SourceData,
): Module;

type RuntimeParams = {
  otherChunks: ChunkData[];
  runtimeModuleIds: ModuleId[];
};

type ChunkRegistrationChunk =
  | ChunkPath
  | { getAttribute: (name: string) => string | null }
  | undefined;

type ChunkRegistration = [
  chunkPath: ChunkRegistrationChunk,
  ...([RuntimeParams] | CompressedModuleFactories),
];

type ChunkList = {
  script: ChunkRegistrationChunk;
  chunks: ChunkData[];
  source: "entry" | "dynamic";
};

// SourceType and SourceData are provided by shared/runtime/runtime-utils.ts
interface RuntimeBackend {
  registerChunk: (chunkPath: ChunkPath | ChunkScript, params?: RuntimeParams) => void;
  /**
   * Returns the same Promise for the same chunk URL.
   */
  loadChunkCached: (
    sourceType: SourceType,
    chunkUrl: ChunkUrl,
  ) => Promise<void>;
}

let BACKEND: RuntimeBackend;

const moduleFactories: ModuleFactories = new Map();
contextPrototype.M = moduleFactories;

/**
 * Returns an absolute url to an asset.
 */
function resolvePathFromModule(
  this: TurbopackBaseContext<Module>,
  moduleId: string,
): string {
  const exported = this.r(moduleId);
  return exported?.default ?? exported;
}
browserContextPrototype.R = resolvePathFromModule;

/**
 * no-op for browser
 * @param modulePath
 */
function resolveAbsolutePath(modulePath?: string): string {
  return `/ROOT/${modulePath ?? ""}`;
}
browserContextPrototype.P = resolveAbsolutePath;

/**
 * Instantiates a runtime module.
 */
function instantiateRuntimeModule(
  moduleId: ModuleId,
  chunkPath: ChunkPath,
): Module {
  return instantiateModule(moduleId, SourceType.Runtime, chunkPath);
}
