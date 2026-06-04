import {
  type BundleOptions,
  compatExternals,
  compatOptionsFromWebpack,
  type ExternalsConfig,
  type WebpackCompatExternalRequest,
  type WebpackConfig,
  type WebpackEntry,
  type WebpackExternals,
} from "@utoo/pack-shared";
import fs from "fs";
import path from "path";
import { readWebpackConfig } from "./readWebpackConfig";

export {
  compatOptionsFromWebpack,
  type WebpackConfig,
} from "@utoo/pack-shared";
export { readWebpackConfig } from "./readWebpackConfig";

export function resolveBundleOptions(
  options: BundleOptions | WebpackConfig,
  projectPath?: string,
  rootPath?: string,
): BundleOptions {
  if ((<WebpackConfig>options).webpackMode) {
    let webpackConfig = <WebpackConfig>options;
    const loadedConfig = readWebpackConfig(projectPath, rootPath);
    webpackConfig = { ...loadedConfig, ...webpackConfig };
    try {
      return compatOptionsFromWebpack(webpackConfig, {
        externalRequests: hasFunctionalExternals(webpackConfig.externals)
          ? collectExternalRequestCandidates(
              getProjectPath(projectPath, rootPath),
              collectWebpackEntryImports(webpackConfig.entry),
            )
          : [],
      });
    } catch (e) {
      throw new Error("Error converting webpack config: " + e);
    }
  } else {
    return materializeFunctionalExternals(
      <BundleOptions>options,
      projectPath,
      rootPath,
    );
  }
}

function materializeFunctionalExternals(
  options: BundleOptions,
  projectPath?: string,
  rootPath?: string,
): BundleOptions {
  const externals = options.config.externals;
  if (!hasFunctionalExternals(externals)) {
    return options;
  }

  const projectDir = getProjectPath(projectPath, rootPath);
  const externalRequests = collectExternalRequestCandidates(
    projectDir,
    options.config.entry?.map((entry) => entry.import) ?? [],
  );

  return {
    ...options,
    config: {
      ...options.config,
      externals: materializeExternalsConfig(externals, externalRequests),
    },
  };
}

function materializeExternalsConfig(
  externals: ExternalsConfig | undefined,
  externalRequests: WebpackCompatExternalRequest[],
): ExternalsConfig | undefined {
  if (typeof externals === "function" || Array.isArray(externals)) {
    return compatExternals(
      externals as WebpackExternals,
      undefined,
      externalRequests,
    );
  }
  return externals;
}

function hasFunctionalExternals(externals: unknown): boolean {
  return (
    typeof externals === "function" ||
    (Array.isArray(externals) &&
      externals.some((external) => typeof external === "function"))
  );
}

function getProjectPath(projectPath?: string, rootPath?: string): string {
  if (!projectPath) {
    return process.cwd();
  }
  if (path.isAbsolute(projectPath)) {
    return projectPath;
  }
  return path.resolve(rootPath ?? process.cwd(), projectPath);
}

function collectWebpackEntryImports(entry: WebpackEntry | undefined): string[] {
  if (!entry) {
    return [];
  }
  if (typeof entry === "string") {
    return [entry];
  }
  if (Array.isArray(entry)) {
    return filterStringEntries(entry);
  }
  return Object.values(entry).flatMap((value) => {
    if (typeof value === "string") {
      return [value];
    }
    if (Array.isArray(value)) {
      return filterStringEntries(value);
    }
    return value?.import ? [value.import] : [];
  });
}

function filterStringEntries(entries: unknown[]): string[] {
  return entries.filter(
    (entry): entry is string => typeof entry === "string" && entry.length > 0,
  );
}

const SOURCE_EXTENSIONS = [
  "",
  ".ts",
  ".tsx",
  ".js",
  ".jsx",
  ".mjs",
  ".cjs",
  ".json",
];
const IMPORT_REQUEST_RE = new RegExp(
  [
    "\\bimport\\s+(?:type\\s+)?(?:[^'\"]*?\\s+from\\s*)?['\"]([^'\"]+)['\"]",
    "\\bexport\\s+(?:type\\s+)?(?:[^'\"]*?\\s+from\\s*)['\"]([^'\"]+)['\"]",
    "\\bimport\\s*\\(\\s*['\"]([^'\"]+)['\"]\\s*\\)",
    "\\brequire\\s*\\(\\s*['\"]([^'\"]+)['\"]\\s*\\)",
  ].join("|"),
  "g",
);

function collectExternalRequestCandidates(
  projectDir: string,
  entryImports: string[],
): WebpackCompatExternalRequest[] {
  const candidates = new Map<string, WebpackCompatExternalRequest>();
  const visited = new Set<string>();

  const addCandidate = (
    request: string,
    context: string,
    dependencyType?: string,
  ) => {
    if (!request) {
      return;
    }
    const key = getExternalRequestCandidateKey(request, context);
    if (candidates.has(key)) {
      return;
    }
    candidates.set(key, { request, context, dependencyType });
  };

  for (const request of readPackageDependencyNames(projectDir)) {
    addCandidate(request, projectDir);
  }

  for (const entryImport of entryImports) {
    addCandidate(entryImport, projectDir, "entry");
    const entryFile = resolveSourceFile(entryImport, projectDir);
    if (entryFile) {
      scanSourceFile(entryFile, candidates, visited);
    }
  }

  return [...candidates.values()];
}

function readPackageDependencyNames(projectDir: string): string[] {
  const packageJsonPath = path.join(projectDir, "package.json");
  if (!fs.existsSync(packageJsonPath)) {
    return [];
  }

  try {
    const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
    return [
      "dependencies",
      "peerDependencies",
      "optionalDependencies",
      "devDependencies",
    ].flatMap((field) =>
      packageJson[field] && typeof packageJson[field] === "object"
        ? Object.keys(packageJson[field])
        : [],
    );
  } catch {
    return [];
  }
}

function scanSourceFile(
  filePath: string,
  candidates: Map<string, WebpackCompatExternalRequest>,
  visited: Set<string>,
) {
  if (visited.has(filePath) || visited.size > 2000) {
    return;
  }
  visited.add(filePath);

  let source: string;
  try {
    source = fs.readFileSync(filePath, "utf8");
  } catch {
    return;
  }

  const context = path.dirname(filePath);
  IMPORT_REQUEST_RE.lastIndex = 0;
  for (const match of source.matchAll(IMPORT_REQUEST_RE)) {
    const request = match[1] || match[2] || match[3] || match[4];
    const dependencyType = match[3] ? "import" : match[4] ? "commonjs" : "esm";
    if (!request) {
      continue;
    }
    const key = getExternalRequestCandidateKey(request, context);
    if (!candidates.has(key)) {
      candidates.set(key, { request, context, dependencyType });
    }
    if (isRelativeOrAbsoluteRequest(request)) {
      const resolved = resolveSourceFile(request, context);
      if (resolved) {
        scanSourceFile(resolved, candidates, visited);
      }
    }
  }
}

function getExternalRequestCandidateKey(
  request: string,
  context: string,
): string {
  return `${context}::${request}`;
}

function isRelativeOrAbsoluteRequest(request: string): boolean {
  return (
    request.startsWith(".") ||
    request.startsWith("/") ||
    /^[A-Za-z]:[\\/]/.test(request)
  );
}

function resolveSourceFile(
  request: string,
  context: string,
): string | undefined {
  if (!isRelativeOrAbsoluteRequest(request)) {
    return undefined;
  }

  const base = path.resolve(context, request);
  for (const ext of SOURCE_EXTENSIONS) {
    const candidate = base + ext;
    if (isFile(candidate)) {
      return candidate;
    }
  }
  for (const ext of SOURCE_EXTENSIONS.slice(1)) {
    const candidate = path.join(base, `index${ext}`);
    if (isFile(candidate)) {
      return candidate;
    }
  }
  return undefined;
}

function isFile(filePath: string): boolean {
  try {
    return fs.statSync(filePath).isFile();
  } catch {
    return false;
  }
}
