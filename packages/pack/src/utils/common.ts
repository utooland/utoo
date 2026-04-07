import path from "path";

export {
  debounce,
  isWellKnownError,
  ModuleBuildError,
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

/**
 * Pack 根目录（pack 包所在目录）。
 * - CJS：使用 __dirname。
 * - ESM：在构建后由脚本注入 __dirname（基于 import.meta.url），此处仅用 __dirname 以保持 CJS 产物不含 import.meta，避免 Node 将 .js 判为 ESM。
 */
export function getPackPath() {
  return path.resolve(__dirname, "../..");
}
