/**
 * Re-export upstream shared runtime types and add utoo-specific extensions.
 *
 * Upstream types are brought in via reference path; we only define
 * utoo-specific additions here.
 */

/// <reference path="../../../../../next.js/turbopack/crates/turbopack-ecmascript-runtime/js/src/shared/runtime/runtime-types.d.ts" />

// utoo-specific: GetWorkerBlobURL (not in upstream)
type GetWorkerBlobURL = (chunks: ChunkPath[]) => string;
