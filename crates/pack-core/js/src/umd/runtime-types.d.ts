/**
 * This file contains runtime types that are shared between all TurboPack
 * ECMAScript runtimes.
 *
 * It is separate from `runtime-utils.ts` because it can be used outside of
 * runtime code, hence it should not contain any function declarations that are
 * specific to the runtime context.
 */

type CurrentScript = { getAttribute: (name: string) => string | null };
type ChunkListPath = string & { readonly brand: unique symbol };
type ChunkListScript = CurrentScript & { readonly brand: unique symbol };
type ChunkPath = string & { readonly brand: unique symbol };
type ChunkScript = CurrentScript & { readonly brand: unique symbol };
type ChunkUrl = string & { readonly brand: unique symbol };
// The dependency specifier when importing externals
type DependencySpecifier = string;
// This is a string in development and a number in production (both arbitrary, implementation defined)
type ModuleId = string | number;

interface Exports {
  __esModule?: boolean;

  [key: string]: any;
}

type ChunkData =
  | ChunkPath
  | {
      path: ChunkPath;
      included: ModuleId[];
      excluded: ModuleId[];
      moduleChunks: ChunkPath[];
    };

type CommonJsRequire = (moduleId: ModuleId) => Exports;
type RuntimeRequire = (request: string) => Exports;
type ModuleContextFactory = (map: ModuleContextMap) => ModuleContext;
type EsmImport = (
  moduleId: ModuleId,
  allowExportDefault: boolean,
) => EsmNamespaceObject | Promise<EsmNamespaceObject>;
type InvokeAsyncLoader = (moduleId: ModuleId) => Promise<Exports>;
type EsmExport = (bindings: any, id: ModuleId | undefined) => void;
type ExportValue = (value: any, id: ModuleId | undefined) => void;
type ExportNamespace = (namespace: any, id: ModuleId | undefined) => void;
type DynamicExport = (
  object: Record<string, any>,
  id: ModuleId | undefined,
) => void;

type LoadChunkByUrl = (chunkUrl: ChunkUrl) => Promise<any> | undefined;
type LoadScript = (scriptUrl: string) => Promise<void>;

type ModuleCache<M> = Record<ModuleId, M>;
// TODO properly type values here
type ModuleFactories = Map<ModuleId, Function>;
// The value is an array with scope hoisting
type CompressedModuleFactories = Record<
  ModuleId,
  Function | [Function, ModuleId[]]
>;

type RelativeURL = (inputUrl: string) => void;
type ResolvePathFromModule = (moduleId: string) => string;

type AsyncModule = (
  body: (
    handleAsyncDependencies: (
      deps: Dep[],
    ) => Exports[] | Promise<() => Exports[]>,
    asyncResult: (err?: any) => void,
  ) => void,
  hasAwait: boolean,
) => void;

type ResolveAbsolutePath = (modulePath?: string) => string;
type GetWorkerBlobURL = (chunks: ChunkPath[]) => string;
type GetPublicPath = () => string;

type ExternalRequire = (
  id: DependencySpecifier,
  thunk: () => any,
  esm?: boolean,
) => Exports | EsmNamespaceObject;
type ExternalImport = (
  id: DependencySpecifier,
) => Promise<Exports | EsmNamespaceObject>;

interface Module {
  exports: Function | Exports | Promise<Exports> | AsyncModulePromise;
  error: Error | undefined;
  id: ModuleId;
  namespaceObject?:
    | EsmNamespaceObject
    | Promise<EsmNamespaceObject>
    | AsyncModulePromise<EsmNamespaceObject>;
}

interface ModuleWithDirection extends Module {
  children: ModuleId[];
  parents: ModuleId[];
}

interface TurbopackBaseContext<M> {
  a: AsyncModule;
  e: Exports;
  r: CommonJsRequire;
  t: RuntimeRequire;
  f: ModuleContextFactory;
  i: EsmImport;
  A: InvokeAsyncLoader;
  s: EsmExport;
  j: DynamicExport;
  v: ExportValue;
  n: ExportNamespace;
  m: Module;
  L: LoadChunkByUrl;
  S: LoadScript;
  c: ModuleCache<M>;
  M: ModuleFactories;
  g: typeof globalThis;
  P: ResolveAbsolutePath;
  U: RelativeURL;
  p: GetPublicPath;
  x: ExternalRequire;
  y: ExternalImport;
  z: CommonJsRequire;
}
