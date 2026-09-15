import external from "external-value";
import asset from "./asset.svg";

export { asset, external };
export const read = (value) => value?.answer ?? 42;
export const flag = 0b001;
export const load = () => import("./lazy").then((module) => module.answer);
export function last(node) {
  do {
    node = node.next;
  } while (node.next);
  return node.value;
}

export function globals(self, __utoo_global__) {
  return [globalThis, self, __utoo_global__, { globalThis }];
}

export function localGlobal(globalThis) {
  return { globalThis, property: { globalThis: 7 }.globalThis };
}
