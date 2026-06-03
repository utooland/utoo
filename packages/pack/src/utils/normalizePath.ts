import fs from "fs";
import path from "path";

export function normalizePath(file: string) {
  if (!file) return file;

  let normalized = path.resolve(file);
  try {
    normalized = fs.realpathSync.native(normalized);
  } catch {}

  return path.sep === "\\" ? normalized.replace(/\\/g, "/") : normalized;
}
