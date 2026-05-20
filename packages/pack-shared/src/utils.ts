import {
  DevServerProxy,
  ProxyOptions,
  ProxyRule,
  RustifiedEnv,
} from "./config";
import { formatIssue, Issue } from "./issue";
import { renderStyledStringToErrorAnsi } from "./styledString";

export class ModuleBuildError extends Error {
  name = "ModuleBuildError";
}

export interface ResultWithIssues {
  issues: Issue[];
}

type IssueKey = `${Issue["severity"]}-${Issue["filePath"]}-${string}-${string}`;
export type IssuesMap = Map<IssueKey, Issue>;
export type EntryIssuesMap = Map<string, IssuesMap>;

export function getIssueKey(issue: Issue): IssueKey {
  return `${issue.severity}-${issue.filePath}-${JSON.stringify(
    issue.title,
  )}-${JSON.stringify(issue.description)}`;
}

export function processIssues(
  result: ResultWithIssues,
  throwIssue: boolean,
  logErrors: boolean,
): void;
export function processIssues(
  currentEntryIssues: EntryIssuesMap,
  key: string,
  result: ResultWithIssues,
  throwIssue: boolean,
  logErrors: boolean,
): void;
export function processIssues(
  resultOrCurrentEntryIssues: ResultWithIssues | EntryIssuesMap,
  throwIssueOrKey: boolean | string,
  logErrorsOrResult: boolean | ResultWithIssues,
  maybeThrowIssue?: boolean,
  maybeLogErrors?: boolean,
) {
  const currentEntryIssues =
    resultOrCurrentEntryIssues instanceof Map
      ? resultOrCurrentEntryIssues
      : undefined;
  const key =
    resultOrCurrentEntryIssues instanceof Map
      ? (throwIssueOrKey as string)
      : undefined;
  const result = currentEntryIssues
    ? (logErrorsOrResult as ResultWithIssues)
    : (resultOrCurrentEntryIssues as ResultWithIssues);
  const throwIssue = currentEntryIssues
    ? maybeThrowIssue!
    : (throwIssueOrKey as boolean);
  const logErrors = currentEntryIssues
    ? maybeLogErrors!
    : (logErrorsOrResult as boolean);
  const newIssues = currentEntryIssues ? new Map<IssueKey, Issue>() : undefined;

  if (currentEntryIssues && key) {
    currentEntryIssues.set(key, newIssues!);
  }

  const relevantIssues = new Set();

  for (const issue of result.issues) {
    if (
      issue.severity !== "error" &&
      issue.severity !== "fatal" &&
      issue.severity !== "warning"
    )
      continue;

    newIssues?.set(getIssueKey(issue), issue);

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

export function isWellKnownError(issue: Issue): boolean {
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

export function rustifyEnv(env: Record<string, string>): RustifiedEnv {
  return Object.entries(env)
    .filter(([_, value]) => value != null)
    .map(([name, value]) => ({
      name,
      value,
    }));
}

/**
 * Convert object-style proxy config into DevServerProxy array.
 * Object keys become context; string value → target + default changeOrigin; object value merged into ProxyRule.
 *
 * @example
 * proxy: [
 *   ...proxyFromObject({
 *     "/api": "http://localhost:3000",
 *     "/auth": { target: "http://localhost:5000", changeOrigin: true },
 *   }),
 * ];
 */
export function proxyFromObject(
  obj: Record<string, string | ProxyOptions>,
): DevServerProxy {
  const rules: ProxyRule[] = [];

  for (const [context, value] of Object.entries(obj)) {
    if (!value) continue;

    if (typeof value === "string") {
      rules.push({
        context,
        target: value,
        changeOrigin: true,
      });
    } else {
      rules.push({
        context,
        changeOrigin: true,
        ...value,
      });
    }
  }

  return rules;
}

type AnyFunc<T> = (this: T, ...args: any) => any;
export function debounce<T, F extends AnyFunc<T>>(
  fn: F,
  ms: number,
  maxWait = Infinity,
) {
  let timeoutId: any;

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
