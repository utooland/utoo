import legacy from "./legacy.cjs";
import dynamic from "./dynamic.cjs";

export { legacy };
export { dynamic };
export * from "./dynamic.cjs";
export * from "./function.cjs";
export * from "./primitive.cjs";
export const staticValue = "explicit";

export function updateDynamic() {
  dynamic.dynamicValue = 42;
  dynamic.addedLater = 99;
}

export function last(node) {
  do {
    node = node.next;
  } while (node.next);
  return node.value;
}

export function makeReader(value) {
  const holder = {
    value,
    read() {
      return () => this.value;
    },
  };
  return holder.read();
}

export function collect(values) {
  const result = [];
  for (const value of values) result.push(value);
  return result;
}

export function collectReturned(getValues) {
  const result = [];
  for (const value of getValues()) result.push(value);
  return result;
}

export function first(values) {
  for (const value of values) return value;
}

export function throwFromLoop(values, error) {
  for (const value of values) {
    if (value) throw error;
  }
}

export function explicitIterator(values) {
  return values[Symbol.iterator]();
}
