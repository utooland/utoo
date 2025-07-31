/**
 * This file contains the runtime code specific to the Turbopack development
 * ECMAScript DOM runtime.
 *
 * It will be appended to the base development runtime code.
 */

/* eslint-disable @typescript-eslint/no-unused-vars */

/// <reference path="./runtime-base.ts" />
/// <reference path="./runtime-types.d.ts" />
/// <reference path="./base-externals-utils.ts" />
  
function augmentContext(context: TurbopackBaseContext<Module>): TurbopackBaseContext<Module> {
  context.x = externalRequire;
  context.y = externalImport;
  return context;
}
