import browserslist from "browserslist";

export function isNodeTarget(target?: string) {
  if (target === undefined) return false;

  try {
    const [distribution] = browserslist(target.split(","), {
      ignoreUnknownVersions: true,
    });
    return distribution?.startsWith("node ") ?? false;
  } catch {
    // Match Config::platform(): `node` is also the explicit fallback when the
    // target isn't a valid Browserslist query.
    return target === "node";
  }
}
