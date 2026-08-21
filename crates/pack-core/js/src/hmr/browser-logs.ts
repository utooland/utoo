type BrowserToTerminal = boolean | "error" | "warn";
type BrowserLogMethod =
  | "assert"
  | "debug"
  | "dir"
  | "dirxml"
  | "error"
  | "group"
  | "groupCollapsed"
  | "groupEnd"
  | "info"
  | "log"
  | "table"
  | "trace"
  | "warn";

interface BrowserLogEntry {
  method: BrowserLogMethod;
  kind: "console" | "uncaught-error" | "unhandled-rejection";
  args: string[];
  stack?: string;
}

interface BrowserLogsMessage {
  event: "browser-logs";
  entries: BrowserLogEntry[];
}

interface BrowserLogForwardingOptions {
  addMessageListener(
    callback: (event: {
      type: string;
      browserToTerminal?: BrowserToTerminal;
    }) => void,
  ): void;
  sendMessage(data: BrowserLogsMessage): boolean;
}

const CONSOLE_METHODS: BrowserLogMethod[] = [
  "assert",
  "log",
  "info",
  "warn",
  "error",
  "debug",
  "table",
  "trace",
  "dir",
  "dirxml",
  "group",
  "groupCollapsed",
  "groupEnd",
];
const MAX_DEPTH = 5;
const MAX_ITEMS = 100;
const MAX_STRING_LENGTH = 10_000;
const MAX_QUEUED_ENTRIES = 1_000;

let initialized = false;

export function initializeBrowserLogForwarding(
  options: BrowserLogForwardingOptions,
): void {
  if (initialized || typeof window === "undefined") return;
  initialized = true;

  let level: BrowserToTerminal | undefined;
  let flushScheduled = false;
  const entries: BrowserLogEntry[] = [];

  const flush = () => {
    flushScheduled = false;
    if (level === undefined || level === false || entries.length === 0) return;
    const forwarded = entries.filter((entry) =>
      shouldForward(level, entry.method),
    );
    entries.length = 0;
    if (forwarded.length === 0) return;
    if (!options.sendMessage({ event: "browser-logs", entries: forwarded })) {
      entries.push(...forwarded);
    }
  };

  const scheduleFlush = () => {
    if (flushScheduled || level === undefined || level === false) return;
    flushScheduled = true;
    if (typeof requestAnimationFrame === "function") {
      requestAnimationFrame(flush);
    } else {
      setTimeout(flush, 0);
    }
  };

  const enqueue = (entry: BrowserLogEntry) => {
    if (
      level === false ||
      (level !== undefined && !shouldForward(level, entry.method))
    ) {
      return;
    }
    if (entries.length === MAX_QUEUED_ENTRIES) entries.shift();
    entries.push(entry);
    scheduleFlush();
  };

  const browserConsole = console as unknown as Record<
    BrowserLogMethod,
    (...args: unknown[]) => void
  >;
  for (const method of CONSOLE_METHODS) {
    const original = browserConsole[method].bind(console);
    browserConsole[method] = (...args: unknown[]) => {
      original(...args);
      if (method === "assert" && args[0]) return;
      if (isInternalHmrLog(args)) return;
      const forwardedArgs = method === "assert" ? args.slice(1) : args;
      enqueue({
        method,
        kind: "console",
        args: forwardedArgs
          .slice(0, MAX_ITEMS)
          .map((arg) => serializeValue(arg)),
        stack: captureStack(2),
      });
    };
  }

  window.addEventListener("error", (event) => {
    const error = event.error instanceof Error ? event.error : undefined;
    enqueue({
      method: "error",
      kind: "uncaught-error",
      args: [
        error
          ? `Uncaught ${error.name}: ${error.message}`
          : `Uncaught Error: ${event.message}`,
      ],
      stack: error?.stack,
    });
  });

  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason;
    enqueue({
      method: "error",
      kind: "unhandled-rejection",
      args: [
        reason instanceof Error
          ? `Unhandled Promise rejection: ${reason.name}: ${reason.message}`
          : `Unhandled Promise rejection: ${serializeValue(reason)}`,
      ],
      stack: reason instanceof Error ? reason.stack : undefined,
    });
  });

  options.addMessageListener((event) => {
    if (
      event.type !== "turbopack-connected" ||
      event.browserToTerminal === undefined
    ) {
      return;
    }
    level = event.browserToTerminal;
    if (level === false) entries.length = 0;
    else scheduleFlush();
  });
}

export function shouldForward(
  level: BrowserToTerminal,
  method: BrowserLogMethod,
): boolean {
  if (level === false) return false;
  if (level === true) return true;
  if (level === "warn")
    return method === "warn" || method === "error" || method === "assert";
  return method === "error" || method === "assert";
}

export function serializeValue(value: unknown): string {
  return serialize(value, 0, new WeakSet<object>());
}

function serialize(
  value: unknown,
  depth: number,
  ancestors: WeakSet<object>,
): string {
  if (typeof value === "string") return truncate(value);
  if (value === undefined) return "undefined";
  if (
    value === null ||
    typeof value === "number" ||
    typeof value === "boolean"
  ) {
    return String(value);
  }
  if (typeof value === "bigint") return `${value}n`;
  if (typeof value === "symbol") return String(value);
  if (typeof value === "function")
    return `[Function ${value.name || "anonymous"}]`;
  if (value instanceof Error) return `${value.name}: ${value.message}`;
  if (value instanceof Date) return value.toISOString();
  if (value instanceof RegExp) return String(value);
  if (typeof value !== "object") return String(value);
  if (ancestors.has(value)) return "[Circular]";
  if (depth >= MAX_DEPTH) return Array.isArray(value) ? "[Array]" : "[Object]";

  ancestors.add(value);
  try {
    if (Array.isArray(value)) {
      const items = value
        .slice(0, MAX_ITEMS)
        .map((item) => serialize(item, depth + 1, ancestors));
      if (value.length > MAX_ITEMS)
        items.push(`… ${value.length - MAX_ITEMS} more`);
      return `[${items.join(", ")}]`;
    }

    const keys = Object.keys(value).slice(0, MAX_ITEMS);
    const items = keys.map((key) => {
      const descriptor = Object.getOwnPropertyDescriptor(value, key);
      const serialized =
        descriptor && "value" in descriptor
          ? serialize(descriptor.value, depth + 1, ancestors)
          : "[Getter]";
      return `${key}: ${serialized}`;
    });
    const remaining = Object.keys(value).length - keys.length;
    if (remaining > 0) items.push(`… ${remaining} more`);
    return `{ ${items.join(", ")} }`;
  } catch {
    return "[Unserializable]";
  } finally {
    ancestors.delete(value);
  }
}

function truncate(value: string): string {
  return value.length > MAX_STRING_LENGTH
    ? `${value.slice(0, MAX_STRING_LENGTH)}…`
    : value;
}

function captureStack(framesToSkip: number): string | undefined {
  const stack = new Error().stack;
  if (!stack) return undefined;
  return stack
    .split("\n")
    .slice(framesToSkip + 1)
    .join("\n");
}

function isInternalHmrLog(args: unknown[]): boolean {
  return typeof args[0] === "string" && args[0].startsWith("[HMR]");
}
