export { value } from "./value.js";

export function loadExternal() {
  return import("script-value").then((module) => module.default);
}
