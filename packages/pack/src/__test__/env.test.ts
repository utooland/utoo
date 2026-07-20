import { describe, expect, it } from "vitest";
import {
  normalizeBuildTurbopackMemoryEviction,
  normalizeDevTurbopackMemoryEviction,
  shouldShutdownDevTurbopackProject,
  shouldUseDevTurbopackBackgroundPersistence,
} from "../utils/env";

describe("normalizeDevTurbopackMemoryEviction", () => {
  it.each([undefined, "1", "true"])(
    "keeps the existing full default when config is unset and env is %s",
    (rawEnv) => {
      expect(normalizeDevTurbopackMemoryEviction(undefined, rawEnv)).toBe(
        "full",
      );
    },
  );

  it.each(["0", "false"])(
    "allows env=%s to explicitly disable eviction",
    (rawEnv) => {
      expect(normalizeDevTurbopackMemoryEviction(undefined, rawEnv)).toBe(
        "off",
      );
    },
  );

  it.each([true, "full"] as const)(
    "allows config=%s to explicitly enable full eviction",
    (value) => {
      expect(normalizeDevTurbopackMemoryEviction(value, "0")).toBe("full");
    },
  );

  it("keeps an explicit false config authoritative over env", () => {
    expect(normalizeDevTurbopackMemoryEviction(false, "1")).toBe("off");
  });
});

describe("normalizeBuildTurbopackMemoryEviction", () => {
  it.each([undefined, "1", "true"])(
    "keeps the existing full default when config is unset and env is %s",
    (rawEnv) => {
      expect(normalizeBuildTurbopackMemoryEviction(undefined, rawEnv)).toBe(
        "full",
      );
    },
  );

  it.each(["0", "false"])("keeps env=%s as an explicit opt-out", (rawEnv) => {
    expect(normalizeBuildTurbopackMemoryEviction(undefined, rawEnv)).toBe(
      "off",
    );
  });

  it("keeps an explicit false config authoritative over env", () => {
    expect(normalizeBuildTurbopackMemoryEviction(false, "1")).toBe("off");
  });
});

describe("shouldUseDevTurbopackBackgroundPersistence", () => {
  it.each([undefined, "1", "true"])(
    "keeps periodic background snapshots by default when env is %s",
    (rawEnv) => {
      expect(
        shouldUseDevTurbopackBackgroundPersistence(undefined, rawEnv),
      ).toBe(true);
    },
  );

  it.each(["0", "false"])(
    "allows env=%s to opt into shutdown-only persistence",
    (rawEnv) => {
      expect(
        shouldUseDevTurbopackBackgroundPersistence(undefined, rawEnv),
      ).toBe(false);
    },
  );

  it("keeps an explicit false config authoritative over env", () => {
    expect(shouldUseDevTurbopackBackgroundPersistence(false, "1")).toBe(false);
  });

  it("keeps an explicit true config authoritative over env", () => {
    expect(shouldUseDevTurbopackBackgroundPersistence(true, "0")).toBe(true);
  });

  it("treats schema null as an unset optional value", () => {
    expect(shouldUseDevTurbopackBackgroundPersistence(null, undefined)).toBe(
      true,
    );
    expect(shouldUseDevTurbopackBackgroundPersistence(null, "0")).toBe(false);
  });
});

describe("shouldShutdownDevTurbopackProject", () => {
  it.each([
    [undefined, true, true, false],
    [true, true, true, true],
    [undefined, true, false, true],
    [false, false, false, false],
  ] as const)(
    "uses configured=%s normalized=%s background=%s => %s",
    (configured, normalized, background, expected) => {
      expect(
        shouldShutdownDevTurbopackProject(configured, normalized, background),
      ).toBe(expected);
    },
  );
});
