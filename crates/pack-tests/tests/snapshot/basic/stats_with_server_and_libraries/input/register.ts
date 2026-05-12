export const RUNTIME_ACTIONS = new Map<string, Function>();

export function registerServerReference(action: Function, id: string, name: string) {
  console.log(`[Register] Action ${name} (${id}) registered.`);
  RUNTIME_ACTIONS.set(id, action);
}
