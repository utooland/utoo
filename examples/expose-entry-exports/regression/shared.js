globalThis.__entryExportSharedExecutions =
  (globalThis.__entryExportSharedExecutions || 0) + 1;

export const executions = globalThis.__entryExportSharedExecutions;
export let live = 1;

export function increment() {
  live += 1;
}
