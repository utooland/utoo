import { describe, expect, it } from "vitest";
import { isNodeTarget } from "./target";

describe("isNodeTarget", () => {
  it.each(["node", "current node", "maintained node versions", "node >= 20"])(
    "recognizes the Node target %s",
    (target) => {
      expect(isNodeTarget(target)).toBe(true);
    },
  );

  it.each([undefined, "web", "last 1 Chrome versions"])(
    "keeps the web target %s on the client watcher",
    (target) => {
      expect(isNodeTarget(target)).toBe(false);
    },
  );

  it("matches the first resolved distribution for mixed queries", () => {
    expect(isNodeTarget("last 1 Chrome versions, current node")).toBe(false);
  });
});
