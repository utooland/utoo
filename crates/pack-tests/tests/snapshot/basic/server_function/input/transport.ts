export function createServerReference(id: string, name: string) {
  return async function (...args: any[]) {
    console.log(`[Transport] Call ${name} (${id}) with`, args);
    return { ok: true };
  };
}
