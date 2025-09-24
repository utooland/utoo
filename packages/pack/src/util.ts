import { codeFrameColumns } from "@babel/code-frame";
import { bold, green, magenta, red } from "picocolors";

import { NapiIssue } from "./binding";
import {
  decodeMagicIdentifier,
  MAGIC_IDENTIFIER_REGEX,
} from "./magicIdentifier";
import { ConfigComplete, DefineEnv, RustifiedEnv, StyledString } from "./types";
import path from 'path';

export class ModuleBuildError extends Error {
  name = "ModuleBuildError";
}

export function processIssues(
  result: TurbopackResult,
  throwIssue: boolean,
  logErrors: boolean,
) {
  const relevantIssues = new Set();

  for (const issue of result.issues) {
    if (
      issue.severity !== "error" &&
      issue.severity !== "fatal" &&
      issue.severity !== "warning"
    )
      continue;

    if (issue.severity !== "warning") {
      if (throwIssue) {
        const formatted = formatIssue(issue);
        relevantIssues.add(formatted);
      }
      // if we throw the issue it will most likely get handed and logged elsewhere
      else if (logErrors && isWellKnownError(issue)) {
        const formatted = formatIssue(issue);
        console.error(formatted);
      }
    }
  }

  if (relevantIssues.size && throwIssue) {
    throw new ModuleBuildError([...relevantIssues].join("\n\n"));
  }
}

export function isWellKnownError(issue: NapiIssue): boolean {
  const { title } = issue;
  const formattedTitle = renderStyledStringToErrorAnsi(title);
  // TODO: add more well known errors
  if (
    formattedTitle.includes("Module not found") ||
    formattedTitle.includes("Unknown module type")
  ) {
    return true;
  }

  return false;
}

export function formatIssue(issue: NapiIssue) {
  const { filePath, title, description, source } = issue;
  let { documentationLink } = issue;
  let formattedTitle = renderStyledStringToErrorAnsi(title).replace(
    /\n/g,
    "\n    ",
  );

  let formattedFilePath = filePath
    .replace("[project]/", "./")
    .replaceAll("/./", "/")
    .replace("\\\\?\\", "");

  let message = "";

  if (source && source.range) {
    const { start } = source.range;
    message = `${formattedFilePath}:${start.line + 1}:${
      start.column + 1
    }\n${formattedTitle}`;
  } else if (formattedFilePath) {
    message = `${formattedFilePath}\n${formattedTitle}`;
  } else {
    message = formattedTitle;
  }
  message += "\n";

  if (source?.range && source.source.content) {
    const { start, end } = source.range;

    message +=
      codeFrameColumns(
        source.source.content,
        {
          start: {
            line: start.line + 1,
            column: start.column + 1,
          },
          end: {
            line: end.line + 1,
            column: end.column + 1,
          },
        },
        { forceColor: true },
      ).trim() + "\n\n";
  }

  if (description) {
    message += renderStyledStringToErrorAnsi(description) + "\n\n";
  }

  // TODO: make it possible to enable this for debugging, but not in tests.
  // if (detail) {
  //   message += renderStyledStringToErrorAnsi(detail) + '\n\n'
  // }

  // TODO: Include a trace from the issue.

  if (documentationLink) {
    message += documentationLink + "\n\n";
  }

  return message;
}

export function renderStyledStringToErrorAnsi(string: StyledString): string {
  function decodeMagicIdentifiers(str: string): string {
    return str.replaceAll(MAGIC_IDENTIFIER_REGEX, (ident) => {
      try {
        return magenta(`{${decodeMagicIdentifier(ident)}}`);
      } catch (e) {
        return magenta(`{${ident} (decoding failed: ${e})}`);
      }
    });
  }

  switch (string.type) {
    case "text":
      return decodeMagicIdentifiers(string.value);
    case "strong":
      return bold(red(decodeMagicIdentifiers(string.value)));
    case "code":
      return green(decodeMagicIdentifiers(string.value));
    case "line":
      return string.value.map(renderStyledStringToErrorAnsi).join("");
    case "stack":
      return string.value.map(renderStyledStringToErrorAnsi).join("\n");
    default:
      throw new Error("Unknown StyledString type", string);
  }
}

export function isRelevantWarning(issue: NapiIssue): boolean {
  return issue.severity === "warning" && !isNodeModulesIssue(issue);
}

function isNodeModulesIssue(issue: NapiIssue): boolean {
  if (issue.severity === "warning" && issue.stage === "config") {
    // Override for the externalize issue
    // `Package foo (serverExternalPackages or default list) can't be external`
    if (
      renderStyledStringToErrorAnsi(issue.title).includes("can't be external")
    ) {
      return false;
    }
  }

  return (
    issue.severity === "warning" &&
    (issue.filePath.match(/^(?:.*[\\/])?node_modules(?:[\\/].*)?$/) !== null ||
      issue.filePath.includes("@utoo/pack"))
  );
}

export function rustifyEnv(env: Record<string, string>): RustifiedEnv {
  return Object.entries(env)
    .filter(([_, value]) => value != null)
    .map(([name, value]) => ({
      name,
      value,
    }));
}

// TODO: extend in future, like SSR support.
interface DefineEnvOptions {
  config: ConfigComplete;
  dev: boolean;
  optionDefineEnv?: DefineEnv;
  // isClient: boolean,
  // isNodeServer: boolean
}

interface Envs {
  [key: string]: string | string[] | boolean;
}

interface SerializedDefineEnv {
  [key: string]: string;
}

export function createDefineEnv(options: DefineEnvOptions): DefineEnv {
  let defineEnv: DefineEnv = options.optionDefineEnv ?? {
    client: [],
    edge: [],
    nodejs: [],
  };

  function getDefineEnv(): SerializedDefineEnv {
    const envs: Envs = {
      "process.env.NODE_ENV": options.dev ? "development" : "production",
    };
    const userDefines = options.config.define ?? {};
    for (const key in userDefines) {
      envs[key] = userDefines[key];
    }

    // serialize
    const defineEnvStringified: SerializedDefineEnv = {};
    for (const key in defineEnv) {
      const value = envs[key];
      defineEnvStringified[key] = JSON.stringify(value);
    }

    return defineEnvStringified;
  }

  // TODO: future define envs need to extends for more compiler like server or edge.
  for (const variant of Object.keys(defineEnv) as (keyof typeof defineEnv)[]) {
    defineEnv[variant] = rustifyEnv(getDefineEnv());
  }

  return defineEnv;
}

type AnyFunc<T> = (this: T, ...args: any) => any;
export function debounce<T, F extends AnyFunc<T>>(
  fn: F,
  ms: number,
  maxWait = Infinity,
) {
  let timeoutId: undefined | NodeJS.Timeout;

  // The time the debouncing function was first called during this debounce queue.
  let startTime = 0;
  // The time the debouncing function was last called.
  let lastCall = 0;

  // The arguments and this context of the last call to the debouncing function.
  let args: Parameters<F>, context: T;

  // A helper used to that either invokes the debounced function, or
  // reschedules the timer if a more recent call was made.
  function run() {
    const now = Date.now();
    const diff = lastCall + ms - now;

    // If the diff is non-positive, then we've waited at least `ms`
    // milliseconds since the last call. Or if we've waited for longer than the
    // max wait time, we must call the debounced function.
    if (diff <= 0 || startTime + maxWait >= now) {
      // It's important to clear the timeout id before invoking the debounced
      // function, in case the function calls the debouncing function again.
      timeoutId = undefined;
      fn.apply(context, args);
    } else {
      // Else, a new call was made after the original timer was scheduled. We
      // didn't clear the timeout (doing so is very slow), so now we need to
      // reschedule the timer for the time difference.
      timeoutId = setTimeout(run, diff);
    }
  }

  return function (this: T, ...passedArgs: Parameters<F>) {
    // The arguments and this context of the most recent call are saved so the
    // debounced function can be invoked with them later.
    args = passedArgs;
    context = this;

    // Instead of constantly clearing and scheduling a timer, we record the
    // time of the last call. If a second call comes in before the timer fires,
    // then we'll reschedule in the run function. Doing this is considerably
    // faster.
    lastCall = Date.now();

    // Only schedule a new timer if we're not currently waiting.
    if (timeoutId === undefined) {
      startTime = lastCall;
      timeoutId = setTimeout(run, ms);
    }
  };
}

// ref:
// https://github.com/vercel/next.js/pull/51883
export function blockStdout() {
  // rust needs stdout to be blocking, otherwise it will throw an error (on macOS at least) when writing a lot of data (logs) to it
  // see https://github.com/napi-rs/napi-rs/issues/1630
  // and https://github.com/nodejs/node/blob/main/doc/api/process.md#a-note-on-process-io
  if ((process.stdout as any)._handle != null) {
    (process.stdout as any)._handle.setBlocking(true);
  }
  if ((process.stderr as any)._handle != null) {
    (process.stderr as any)._handle.setBlocking(true);
  }
}


export function getPackPath() {
  return path.resolve(__dirname, '..')
}