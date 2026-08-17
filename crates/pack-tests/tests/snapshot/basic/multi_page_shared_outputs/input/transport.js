export function createServerReference(id, name) {
  return (...args) => ({ args, id, name });
}
