export function isTruthyEnv(value: string | undefined): boolean {
  return value === "1" || value === "true";
}

export function isPersistentCachingEnabled(
  configuredValue: boolean | undefined,
): boolean {
  return (
    !isTruthyEnv(process.env.DISABLE_PERSISTENT_CACHE?.toLowerCase()) &&
    (configuredValue ?? true)
  );
}

export function normalizeTurbopackMemoryEviction(
  value: boolean | "auto" | "full" | undefined,
): "off" | "auto" | "full" {
  if (value === false) {
    return "off";
  }
  if (value === true || value === "full") {
    return "full";
  }
  if (value === "auto") {
    return "auto";
  }

  const rawEnv = process.env.TURBO_ENGINE_EVICT_AFTER_SNAPSHOT;
  if (rawEnv == null) {
    return "auto";
  }
  return rawEnv === "1" || rawEnv === "true" ? "full" : "off";
}
