import { afterEach, describe, expect, it } from "vitest";
import { isPersistentCachingEnabled } from "./env";

const previousDisablePersistentCache = process.env.DISABLE_PERSISTENT_CACHE;

afterEach(() => {
  if (previousDisablePersistentCache === undefined) {
    delete process.env.DISABLE_PERSISTENT_CACHE;
  } else {
    process.env.DISABLE_PERSISTENT_CACHE = previousDisablePersistentCache;
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

  it.each(["1", "true"])(
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
