import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";
import { changedFiles, classifyChanges, detectChanges } from "./ci-changes.mjs";

// Keep temporary repositories isolated from user-level Git tracing daemons.
process.env.GIT_TRACE2_EVENT = "0";

const expected = (...names) =>
  Object.fromEntries(
    ["pack", "pm", "web", "rust"].map((name) => [name, names.includes(name)]),
  );

for (const [path, names] of [
  ["crates/pack-core/js/src/library/runtime.ts", ["pack", "web", "rust"]],
  ["crates/pack-api/src/lib.rs", ["pack", "web", "rust"]],
  ["crates/pack-schema/src/lib.rs", ["pack", "web", "rust"]],
  ["crates/pack-napi/src/lib.rs", ["pack", "rust"]],
  ["crates/pack-tests/tests/snapshot/input/README.md", ["pack", "rust"]],
  ["crates/pack-tests/tests/snapshot/input/page.md", ["pack", "rust"]],
  ["crates/pm/src/main.rs", ["pm", "rust"]],
  ["crates/ruborist/src/lib.rs", ["pm", "web", "rust"]],
  ["crates/utoo-wasm/src/lib.rs", ["web", "rust"]],
  ["packages/pack-shared/src/config.ts", ["pack", "web"]],
  ["packages/pack/src/index.ts", ["pack"]],
  ["packages/utoo-web/src/index.ts", ["web"]],
  ["examples/utooweb-demo/src/Project.tsx", ["web"]],
  ["e2e/utoo-web/utooweb-demo.spec.ts", ["web"]],
  ["examples/with-react/src/App.tsx", ["pack"]],
  ["e2e/npm-launcher.sh", ["pm"]],
  ["vendor/templates/launcher.utoo.js.template", ["pm"]],
  ["bench/pm-bench.sh", ["pm"]],
  ["next.js", ["pack", "web", "rust"]],
  ["agents/tools/analyze_trace.py", ["pack"]],
  [".github/workflows/pack-ci.yml", ["pack"]],
  [".github/workflows/pm-e2e-bench.yml", ["pm"]],
  [".github/workflows/utooweb-ci.yml", ["web"]],
  [".github/workflows/clippy.yml", ["rust"]],
]) {
  test(`routes ${path}`, () => {
    assert.deepEqual(classifyChanges([path]), expected(...names));
  });
}

test("shared and unknown inputs run all checks", () => {
  for (const path of [
    "Cargo.lock",
    "Cargo.toml",
    "rust-toolchain.toml",
    ".cargo/config.toml",
    ".gitmodules",
    "package.json",
    "package-lock.json",
    "packages/pack/package.json",
    "turbo.json",
    "scripts/add-esm-extensions.cjs",
    ".github/actions/setup-utoo-bin/action.yml",
    ".github/scripts/ci-changes.mjs",
    ".github/workflows/ci-changes.yml",
    "crates/new-crate/src/lib.rs",
  ]) {
    assert.deepEqual(
      classifyChanges([path]),
      expected("pack", "pm", "web", "rust"),
      path,
    );
  }
});

test("documentation does not run heavy checks", () => {
  assert.deepEqual(
    classifyChanges([
      "README.md",
      "AGENTS.md",
      "docs/guide.md",
      "agents/rust-code-guard.md",
    ]),
    expected(),
  );
});

test("changes affecting several products retain their combined coverage", () => {
  assert.deepEqual(
    classifyChanges([
      "packages/pack/src/index.ts",
      "crates/ruborist/src/lib.rs",
    ]),
    expected("pack", "pm", "web", "rust"),
  );
});

function repository(t) {
  const cwd = mkdtempSync(join(tmpdir(), "utoo-ci-changes-"));
  t.after(() => rmSync(cwd, { recursive: true, force: true }));
  const git = (...args) =>
    execFileSync("git", args, {
      cwd,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  git("init", "-b", "base");
  git("config", "user.name", "CI test");
  git("config", "user.email", "ci@example.invalid");
  const commit = (path) => {
    mkdirSync(dirname(join(cwd, path)), { recursive: true });
    writeFileSync(join(cwd, path), path);
    git("add", ".");
    git("commit", "-m", "Test change");
    return git("rev-parse", "HEAD");
  };
  return { cwd, git, commit };
}

test("shallow PR diff includes earlier PR commits and excludes base-only changes", (t) => {
  const { cwd, git, commit } = repository(t);
  commit("README.md");
  git("checkout", "-b", "feature");
  commit("crates/pm/src/new.rs");
  commit("docs/new.md");
  git("checkout", "base");
  commit("packages/pack/src/unrelated.ts");
  git("merge", "--no-ff", "feature", "-m", "PR merge");
  const checkout = join(cwd, "checkout");
  git("clone", "--depth=2", pathToFileURL(cwd).href, checkout);
  assert.deepEqual(
    changedFiles({ eventName: "pull_request", cwd: checkout }).sort(),
    ["crates/pm/src/new.rs", "docs/new.md"],
  );
});

test("push diff includes the full batch, deleted paths and both sides of a rename", (t) => {
  const { cwd, git, commit } = repository(t);
  commit("crates/pm/src/deleted.rs");
  const before = commit("packages/pack/src/moved.ts");
  git("rm", "crates/pm/src/deleted.rs");
  git("commit", "-m", "Delete");
  mkdirSync(join(cwd, "packages/utoo-web/src"), { recursive: true });
  git("mv", "packages/pack/src/moved.ts", "packages/utoo-web/src/moved.ts");
  git("commit", "-m", "Move");
  assert.deepEqual(changedFiles({ eventName: "push", before, cwd }).sort(), [
    "crates/pm/src/deleted.rs",
    "packages/pack/src/moved.ts",
    "packages/utoo-web/src/moved.ts",
  ]);
});

test("uncertain diffs and manual runs fall back to full coverage", (t) => {
  const { cwd, commit } = repository(t);
  commit("README.md");
  for (const options of [
    { eventName: "push", before: "0".repeat(40) },
    { eventName: "push", before: "invalid" },
    { eventName: "push", before: "f".repeat(40) },
    { eventName: "pull_request" },
    { eventName: "workflow_dispatch" },
  ]) {
    assert.deepEqual(
      detectChanges({ ...options, cwd }),
      expected("pack", "pm", "web", "rust"),
    );
  }
});
