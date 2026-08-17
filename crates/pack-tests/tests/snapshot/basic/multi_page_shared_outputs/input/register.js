export function registerServerReference(action, id, name) {
  globalThis.serverActions ??= new Map();
  globalThis.serverActions.set(`${id}:${name}`, action);
}
