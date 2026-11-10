import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const products = ["pack", "pm", "web", "rust"];

// Match executable inputs before documentation: snapshot fixtures can be Markdown.
const routes = [
  [/^next\.js(?:\/|$)/, ["pack", "web", "rust"]],
  [/^crates\/pack-(?:core|api|schema)\//, ["pack", "web", "rust"]],
  [/^crates\/pack-(?:napi|cli|tests)\//, ["pack", "rust"]],
  [/^crates\/pm\//, ["pm", "rust"]],
  [/^crates\/ruborist\//, ["pm", "web", "rust"]],
  [/^crates\/utoo-wasm\//, ["web", "rust"]],
  [/^packages\/pack-shared\//, ["pack", "web"]],
  [/^packages\/pack(?:-cli)?\//, ["pack"]],
  [/^packages\/utoo-web\//, ["web"]],
  [/^(?:examples\/utooweb-demo|e2e\/utoo-web)\//, ["web"]],
  [/^examples\//, ["pack"]],
  [/^(?:e2e|bench|vendor)\//, ["pm"]],
  [/^agents\/tools\//, ["pack"]],
  [/^\.github\/workflows\/pack-/, ["pack"]],
  [/^\.github\/workflows\/pm-/, ["pm"]],
  [/^\.github\/workflows\/utooweb-/, ["web"]],
  [/^\.github\/workflows\/clippy\.yml$/, ["rust"]],
];

function affectedProducts(path) {
  // All workspace installs use the same manifests, even for isolated products.
  if (/(^|\/)package(?:-lock)?\.json$/.test(path)) return products;
  const route = routes.find(([pattern]) => pattern.test(path));
  if (route) return route[1];
  if (
    /^docs\//.test(path) ||
    /^[^/]+\.md$/.test(path) ||
    /^(?:LICENSE|NOTICE)(?:\.[^/]*)?$/.test(path) ||
    /^(?:\.agents|\.claude|\.codex)\//.test(path) ||
    /^agents\/.*\.md$/.test(path) ||
    /^\.github\/(?:ISSUE_TEMPLATE\/|PULL_REQUEST_TEMPLATE\.md$)/.test(path)
  ) {
    return [];
  }
  // Shared config, the detector itself, and unknown inputs conservatively run all.
  return products;
}

export function classifyChanges(paths) {
  const affected = new Set(paths.flatMap(affectedProducts));
  return Object.fromEntries(products.map((name) => [name, affected.has(name)]));
}

export function changedFiles({ eventName, before, sha = "HEAD", cwd }) {
  const git = (...args) =>
    execFileSync("git", args, {
      cwd,
      encoding: "utf8",
      maxBuffer: 32 * 1024 * 1024,
      timeout: 60_000,
      stdio: ["ignore", "pipe", "pipe"],
    });
  let base;
  if (eventName === "pull_request") {
    // Checkout pins github.sha, the tested PR merge commit. Its first parent is
    // the base tree, so this includes the complete PR, not just its latest commit.
    const parents = git("rev-list", "--parents", "-n", "1", sha)
      .trim()
      .split(/\s+/);
    if (parents.length !== 3)
      throw new Error("PR merge parents are unavailable");
    base = parents[1];
  } else if (eventName === "push") {
    if (!/^[a-f0-9]{40}$/i.test(before ?? "") || /^0+$/.test(before)) {
      throw new Error("Push base is unavailable");
    }
    try {
      git("cat-file", "-e", `${before}^{commit}`);
    } catch {
      git("fetch", "--no-tags", "--depth=1", "origin", before);
    }
    base = before;
  } else {
    throw new Error("Manual or unsupported event: run all checks");
  }
  // Treat renames as delete + add so both products run when a file moves.
  return git("diff", "--name-only", "--no-renames", "-z", base, sha, "--")
    .split("\0")
    .filter(Boolean);
}

export function detectChanges(options) {
  try {
    const files = changedFiles(options);
    console.log(`Detected ${files.length} changed paths`);
    return classifyChanges(files);
  } catch (error) {
    console.warn(`Running all checks: ${error.message}`);
    return Object.fromEntries(products.map((name) => [name, true]));
  }
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const result = detectChanges({
    eventName: process.env.CI_EVENT_NAME,
    before: process.env.CI_BEFORE_SHA,
    sha: process.env.GITHUB_SHA || "HEAD",
  });
  const output = Object.entries(result)
    .map(([name, value]) => `${name}=${value}\n`)
    .join("");
  console.log(output);
  if (process.env.GITHUB_OUTPUT)
    appendFileSync(process.env.GITHUB_OUTPUT, output);
}
