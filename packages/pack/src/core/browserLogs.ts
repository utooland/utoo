import { AnyMap, originalPositionFor } from "@jridgewell/trace-mapping";
import type {
  BrowserLogMethod,
  BrowserLogsMessage,
  BrowserToTerminal,
} from "@utoo/pack-shared";
import fs from "fs/promises";
import path from "path";
import { fileURLToPath, pathToFileURL } from "url";
import type { StackFrame } from "../binding";
import type { Project } from "./types";

const BROWSER_LOG_METHODS = new Set<BrowserLogMethod>([
  "assert",
  "debug",
  "dir",
  "dirxml",
  "error",
  "group",
  "groupCollapsed",
  "groupEnd",
  "info",
  "log",
  "table",
  "trace",
  "warn",
]);
const SOURCE_MAP_CACHE_LIMIT = 25;
const SOURCE_MAP_CACHE_TTL_MS = 100;
type FlattenedSourceMap = ReturnType<typeof AnyMap>;
const sourceMapCache = new Map<
  string,
  { expiresAt: number; map: Promise<FlattenedSourceMap | null> }
>();

export function isBrowserLogsMessage(
  value: unknown,
): value is BrowserLogsMessage {
  if (!value || typeof value !== "object") return false;
  const message = value as Partial<BrowserLogsMessage>;
  if (message.event !== "browser-logs" || !Array.isArray(message.entries)) {
    return false;
  }
  if (message.entries.length > 1_000) return false;
  return message.entries.every(
    (entry) =>
      Boolean(entry) &&
      typeof entry === "object" &&
      BROWSER_LOG_METHODS.has(entry.method) &&
      (entry.kind === "console" ||
        entry.kind === "uncaught-error" ||
        entry.kind === "unhandled-rejection") &&
      Array.isArray(entry.args) &&
      entry.args.length <= 100 &&
      entry.args.every((arg) => typeof arg === "string") &&
      (entry.stack === undefined || typeof entry.stack === "string"),
  );
}

export function shouldForwardBrowserLog(
  level: BrowserToTerminal,
  method: BrowserLogMethod,
): boolean {
  if (level === false) return false;
  if (level === true) return true;
  if (level === "warn")
    return method === "warn" || method === "error" || method === "assert";
  return method === "error" || method === "assert";
}

export async function forwardBrowserLogs(
  message: BrowserLogsMessage,
  level: BrowserToTerminal,
  project: Project,
  projectPath: string,
  outputPath: string,
): Promise<void> {
  for (const entry of message.entries) {
    if (!shouldForwardBrowserLog(level, entry.method)) continue;
    const location = entry.stack
      ? await getBrowserLogLocation(
          entry.stack,
          project,
          projectPath,
          outputPath,
        )
      : undefined;
    const suffix = location ? ` (${location})` : "";
    const formatted = `[browser] ${entry.args.join(" ")}${suffix}`;
    if (entry.method === "assert") console.assert(false, formatted);
    else if (entry.method === "trace") console.log(formatted);
    else (console[entry.method] as (message: string) => void)(formatted);
  }
}

async function getBrowserLogLocation(
  stack: string,
  project: Project,
  projectPath: string,
  outputPath: string,
): Promise<string | undefined> {
  const currentDirectoryFileUrl = pathToFileURL(
    `${projectPath}${path.sep}`,
  ).href;
  for (const line of stack.split("\n")) {
    const match =
      line.match(/^\s*at (?:(.*?) \()?(.+):(\d+):(\d+)\)?$/) ??
      line.match(/^\s*(.*?)@(.+):(\d+):(\d+)$/);
    if (!match) continue;
    const [, methodName, file, lineNumber, columnNumber] = match;
    if (file.startsWith("node:") || file.includes("/hmr/browser-logs")) {
      continue;
    }
    const frame: StackFrame = {
      isServer: false,
      file,
      line: Number(lineNumber),
      column: Number(columnNumber),
      ...(methodName ? { methodName } : {}),
    };
    let mapped: StackFrame | null = null;
    for (const candidate of browserFrameCandidates(frame, outputPath)) {
      try {
        mapped = await project.traceSource(candidate, currentDirectoryFileUrl);
      } catch {
        mapped = null;
      }
      mapped ??= await traceWrittenSourceMap(candidate, outputPath);
      if (mapped) break;
    }
    const result = mapped ?? frame;
    return formatBrowserLogLocation(
      result.originalFile ?? result.file,
      result.line,
      result.column,
      projectPath,
    );
  }
  return undefined;
}

async function traceWrittenSourceMap(
  frame: StackFrame,
  outputPath: string,
): Promise<StackFrame | null> {
  if (!frame.file.startsWith("file://") || frame.line === undefined)
    return null;
  let assetFile: string;
  try {
    assetFile = fileURLToPath(frame.file);
  } catch {
    return null;
  }
  const relative = path.relative(outputPath, assetFile);
  if (!relative || relative.startsWith("..") || path.isAbsolute(relative)) {
    return null;
  }

  const sourceMapPath = `${assetFile}.map`;
  try {
    const sourceMap = await loadWrittenSourceMap(sourceMapPath);
    if (!sourceMap) return null;
    const traced = originalPositionFor(sourceMap, {
      line: frame.line,
      column: Math.max(0, (frame.column ?? 1) - 1),
    });
    if (!traced.source || traced.line === null || traced.column === null) {
      return null;
    }
    return {
      ...frame,
      file: traced.source,
      originalFile: traced.source,
      line: traced.line,
      column: traced.column + 1,
    };
  } catch {
    return null;
  }
}

async function loadWrittenSourceMap(
  sourceMapPath: string,
): Promise<FlattenedSourceMap | null> {
  const now = Date.now();
  const cached = sourceMapCache.get(sourceMapPath);
  if (cached && cached.expiresAt > now) return cached.map;

  const map = fs
    .readFile(sourceMapPath, "utf8")
    .then(
      (contents) =>
        new AnyMap(JSON.parse(contents), pathToFileURL(sourceMapPath).href),
    )
    .catch(() => null);
  sourceMapCache.delete(sourceMapPath);
  sourceMapCache.set(sourceMapPath, {
    expiresAt: now + SOURCE_MAP_CACHE_TTL_MS,
    map,
  });
  if (sourceMapCache.size > SOURCE_MAP_CACHE_LIMIT) {
    const oldest = sourceMapCache.keys().next().value;
    if (oldest) sourceMapCache.delete(oldest);
  }
  return map;
}

function browserFrameCandidates(
  frame: StackFrame,
  outputPath: string,
): StackFrame[] {
  if (!frame.file.startsWith("http://") && !frame.file.startsWith("https://")) {
    return [frame];
  }
  try {
    const url = new URL(frame.file);
    const segments = decodeURIComponent(url.pathname)
      .split("/")
      .filter((segment) => segment && segment !== "." && segment !== "..");
    const candidates = segments.map((_, index) => ({
      ...frame,
      file: pathToFileURL(path.join(outputPath, ...segments.slice(index))).href,
    }));
    return [...candidates, frame];
  } catch {
    return [frame];
  }
}

function formatBrowserLogLocation(
  file: string,
  line: number | undefined,
  column: number | undefined,
  projectPath: string,
): string {
  let displayFile = file;
  if (file.startsWith("file://")) {
    try {
      displayFile = fileURLToPath(file);
    } catch {}
  }
  if (path.isAbsolute(displayFile)) {
    const relative = path.relative(projectPath, displayFile);
    if (relative && !relative.startsWith("..")) displayFile = relative;
  }
  displayFile = displayFile.replaceAll(path.sep, "/");
  return `${displayFile}${line === undefined ? "" : `:${line}`}${column === undefined ? "" : `:${column}`}`;
}
