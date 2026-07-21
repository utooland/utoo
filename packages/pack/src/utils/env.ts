export function isTruthyEnv(value: string | undefined): boolean {
  return value === "1" || value === "true";
}

export function isPersistentCachingEnabled(
  configuredValue: boolean | undefined,
): boolean {
  return (
    !isTruthyEnv(process.env.DISABLE_PERSISTENT_CACHE) &&
    (configuredValue ?? true)
  );
}

export function normalizeTurbopackMemoryEviction(
  value: boolean | "full" | undefined,
): "off" | "full" {
  if (value === false) {
    return "off";
  }
  if (value === true || value === "full") {
    return "full";
  }

  const rawEnv = process.env.TURBO_ENGINE_EVICT_AFTER_SNAPSHOT;
  return rawEnv == null || rawEnv === "1" || rawEnv === "true" ? "full" : "off";
}
