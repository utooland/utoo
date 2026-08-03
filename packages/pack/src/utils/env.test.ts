import { afterEach, describe, expect, it } from "vitest";
import {
  isPersistentCachingEnabled,
  normalizeTurbopackMemoryEviction,
} from "./env";

const previousDisablePersistentCache = process.env.DISABLE_PERSISTENT_CACHE;
const previousEvictAfterSnapshot =
  process.env.TURBO_ENGINE_EVICT_AFTER_SNAPSHOT;

afterEach(() => {
  if (previousDisablePersistentCache === undefined) {
    delete process.env.DISABLE_PERSISTENT_CACHE;
  } else {
    process.env.DISABLE_PERSISTENT_CACHE = previousDisablePersistentCache;
  }

  if (previousEvictAfterSnapshot === undefined) {
    delete process.env.TURBO_ENGINE_EVICT_AFTER_SNAPSHOT;
  } else {
    process.env.TURBO_ENGINE_EVICT_AFTER_SNAPSHOT = previousEvictAfterSnapshot;
  }
});

describe("isPersistentCachingEnabled", () => {
  it("enables persistent caching by default", () => {
    delete process.env.DISABLE_PERSISTENT_CACHE;

    expect(isPersistentCachingEnabled(undefined)).toBe(true);
  });

  it("preserves an explicitly disabled configuration", () => {
    delete process.env.DISABLE_PERSISTENT_CACHE;

    expect(isPersistentCachingEnabled(false)).toBe(false);
  });

  it.each(["1", "true", "TRUE", "True"])(
    "disables persistent caching when the environment variable is %s",
    (value) => {
      process.env.DISABLE_PERSISTENT_CACHE = value;

      expect(isPersistentCachingEnabled(true)).toBe(false);
      expect(isPersistentCachingEnabled(undefined)).toBe(false);
    },
  );

  it("ignores other environment variable values", () => {
    process.env.DISABLE_PERSISTENT_CACHE = "0";

    expect(isPersistentCachingEnabled(undefined)).toBe(true);
  });
});

describe("normalizeTurbopackMemoryEviction", () => {
  it("uses auto by default", () => {
    delete process.env.TURBO_ENGINE_EVICT_AFTER_SNAPSHOT;

    expect(normalizeTurbopackMemoryEviction(undefined)).toBe("auto");
  });

  it.each([
    [false, "off"],
    [true, "full"],
    ["full", "full"],
    ["auto", "auto"],
  ] as const)("maps an explicit %s value to %s", (value, expected) => {
    process.env.TURBO_ENGINE_EVICT_AFTER_SNAPSHOT = "0";

    expect(normalizeTurbopackMemoryEviction(value)).toBe(expected);
  });

  it.each([
    ["1", "full"],
    ["true", "full"],
    ["0", "off"],
    ["false", "off"],
    ["", "off"],
    ["TRUE", "off"],
  ] as const)(
    "preserves the legacy environment value %s as %s",
    (value, expected) => {
      process.env.TURBO_ENGINE_EVICT_AFTER_SNAPSHOT = value;

      expect(normalizeTurbopackMemoryEviction(undefined)).toBe(expected);
    },
  );
});
