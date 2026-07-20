export function isTruthyEnv(value: string | undefined): boolean {
  return value === "1" || value === "true";
}

export function isPersistentCachingEnabled(
  configuredValue: boolean | null | undefined,
): boolean {
  return (
    !isTruthyEnv(process.env.DISABLE_PERSISTENT_CACHE?.toLowerCase()) &&
    (configuredValue ?? true)
  );
}

export function shouldUseDevTurbopackBackgroundPersistence(
  value: boolean | null | undefined,
  rawEnv: string | undefined,
): boolean {
  if (value != null) {
    return value;
  }
  if (rawEnv === undefined) {
    return true;
  }
  return isTruthyEnv(rawEnv);
}

export function shouldShutdownDevTurbopackProject(
  configuredPersistentCaching: boolean | null | undefined,
  persistentCaching: boolean,
  backgroundPersistence: boolean,
): boolean {
  return (
    persistentCaching &&
    (configuredPersistentCaching === true || !backgroundPersistence)
  );
}

type TurbopackMemoryEvictionConfig = boolean | "full" | undefined;
type TurbopackMemoryEvictionMode = "off" | "full";

function normalizeTurbopackMemoryEviction(
  value: TurbopackMemoryEvictionConfig,
  rawEnv: string | undefined,
  defaultMode: TurbopackMemoryEvictionMode,
): TurbopackMemoryEvictionMode {
  if (value === false) {
    return "off";
  }
  if (value === true || value === "full") {
    return "full";
  }

  if (rawEnv === undefined) {
    return defaultMode;
  }
  return isTruthyEnv(rawEnv) ? "full" : "off";
}

export function normalizeDevTurbopackMemoryEviction(
  value: TurbopackMemoryEvictionConfig,
  rawEnv: string | undefined,
): TurbopackMemoryEvictionMode {
  return normalizeTurbopackMemoryEviction(value, rawEnv, "full");
}

export function normalizeBuildTurbopackMemoryEviction(
  value: TurbopackMemoryEvictionConfig,
  rawEnv: string | undefined,
): TurbopackMemoryEvictionMode {
  return normalizeTurbopackMemoryEviction(value, rawEnv, "full");
}
