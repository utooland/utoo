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

// Provided by build
declare function instantiateModule(
  id: ModuleId,
  sourceType: SourceType,
  sourceData: SourceData,
): Module;

type RuntimeParams = {
  runtimeModuleIds: ModuleId[];
};

// Used by upstream build-base.ts for chunk registration
type ChunkRegistrationChunk =
  | ChunkPath
  | { getAttribute: (name: string) => string | null }
  | undefined;

type ChunkRegistration = [
  chunkPath: ChunkRegistrationChunk,
  ...([RuntimeParams] | CompressedModuleFactories),
];

// SourceType and SourceData are provided by shared/runtime/runtime-utils.ts
interface RuntimeBackend {
  registerChunk: (
    chunkPath: ChunkPath | ChunkScript,
    params?: RuntimeParams,
  ) => void;
}

let BACKEND: RuntimeBackend;

const moduleFactories: ModuleFactories = new Map();
contextPrototype.M = moduleFactories;

/**
 * Determine the chunk to register from a registration entry.
 * In library builds, chunks are always string paths or script objects.
 */
function getChunkFromRegistration(
  chunk: ChunkRegistrationChunk,
): ChunkPath | ChunkScript {
  if (typeof chunk === "string") {
    return chunk;
  } else if (chunk) {
    return { src: chunk.getAttribute("src")! } as unknown as ChunkScript;
  } else {
    throw new Error("chunk path is empty");
  }
}

/**
 * Load CommonJS externals when a UMD bundle runs in a CommonJS environment.
 * Browser-targeted UMD bundles need this too because their wrapper supports
 * both global and CommonJS consumers.
 */
function externalRequire(
  id: ModuleId,
  thunk: () => any,
  esm: boolean = false,
): Exports | EsmNamespaceObject {
  let raw;
  try {
    raw = thunk();
  } catch (err) {
    throw new Error(`Failed to load external module ${id}: ${err}`);
  }

  if (!esm || raw.__esModule) {
    return raw;
  }

  return interopEsm(raw, createNS(raw), true);
}

externalRequire.resolve = (
  id: string,
  options?: {
    paths?: string[];
  },
) => {
  return require.resolve(id, options);
};
contextPrototype.x = externalRequire;
