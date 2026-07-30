module.exports = function externalNamespace(mod) {
  if (mod && mod.__esModule) return mod;

  const ns = Object.create(null);
  const isEsmNamespace =
    mod &&
    typeof Symbol !== "undefined" &&
    Symbol.toStringTag &&
    mod[Symbol.toStringTag] === "Module";

  if (mod && (typeof mod === "object" || typeof mod === "function")) {
    for (const key in mod) {
      if (
        key === "__esModule" ||
        (!isEsmNamespace && key === "default")
      ) {
        continue;
      }

      Object.defineProperty(ns, key, {
        enumerable: true,
        get: () => mod[key],
      });
    }
  }

  if (!isEsmNamespace) {
    Object.defineProperty(ns, "default", { enumerable: true, value: mod });
  }
  Object.defineProperty(ns, "__esModule", { value: true });
  if (typeof Symbol !== "undefined" && Symbol.toStringTag) {
    Object.defineProperty(ns, Symbol.toStringTag, { value: "Module" });
  }

  return ns;
};
