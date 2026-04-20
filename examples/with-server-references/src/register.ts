export const RUNTIME_ACTIONS = new Map();

export function registerServerReference(action: any, id: string, name: string) {
  console.log(`[server-register] Action ${name} (${id}) registered.`);
  RUNTIME_ACTIONS.set(id, action);
}
