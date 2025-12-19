import path from "path";

export {
  ModuleBuildError,
  createDefineEnv,
  debounce,
  isWellKnownError,
  processIssues,
  rustifyEnv,
} from "@utoo/pack-shared";

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
  return path.resolve(__dirname, "..");
}
