import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { setTimeout as sleep } from "node:timers/promises";

const required = (name) => {
  assert(process.env[name], `${name} is required`);
  return path.resolve(process.env[name]);
};
const root = required("BENCHMARK_ROOT");
const results = required("RESULTS_DIRECTORY");
const caseDirectory = path.join(root, "cases/react-dashboard");
const preload = fileURLToPath(new URL("./pack-native-override.cjs", import.meta.url));
const readJson = (file) => JSON.parse(fs.readFileSync(file, "utf8"));
const writeJson = (name, value) => fs.writeFileSync(path.join(results, name), `${JSON.stringify(value, null, 2)}\n`);
const sha256 = (file) => createHash("sha256").update(fs.readFileSync(file)).digest("hex");
const command = (executable, args = []) => execFileSync(executable, args, { cwd: root, encoding: "utf8" }).trim();
const rows = [];
fs.mkdirSync(results, { recursive: true });

function outputSnapshot() {
  const directory = path.join(caseDirectory, "dist");
  const files = [];
  function visit(current) {
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const filename = path.join(current, entry.name);
      if (entry.isDirectory()) visit(filename);
      else if (entry.isFile()) files.push(path.relative(directory, filename));
      else throw new Error(`Unexpected output entry: ${filename}`);
    }
  }
  visit(directory);
  const digest = createHash("sha256");
  let bytes = 0;
  for (const filename of files.sort()) {
    const content = fs.readFileSync(path.join(directory, filename));
    bytes += content.length;
    digest.update(filename).update(content);
  }
  assert(files.length > 0, "Build produced no files");
  return { files: files.length, bytes, sha256: digest.digest("hex") };
}

function cleanCache() {
  // Keep the original benchmark's cache cleanup list and both search roots.
  for (const directory of [root, caseDirectory]) {
    for (const cache of [".parcel-cache", "node_modules/.cache", "node_modules/.vite", "node_modules/.farm", ".turbopack", ".bun"]) {
      fs.rmSync(path.join(directory, cache), { recursive: true, force: true });
    }
  }
}

async function build(native, id) {
  const logPath = path.join(results, `${id}.log`);
  const auditPath = path.join(results, `${id}.native.jsonl`);
  fs.writeFileSync(auditPath, "");
  const log = fs.openSync(logPath, "w");
  const env = {
    ...process.env,
    NODE_OPTIONS: `${process.env.NODE_OPTIONS || ""} --require ${JSON.stringify(preload)}`.trim(),
    PROFILE_NATIVE_LIBRARY: native.path,
    PROFILE_NATIVE_SHA256: native.sha256,
    PROFILE_NATIVE_LOAD_LOG: auditPath,
  };
  delete env.NAPI_RS_NATIVE_LIBRARY_PATH;
  // Match BuildTool.build(): Date.now immediately before launching Node --run.
  const started = Date.now();
  const child = spawn(process.execPath, ["--run", "build:utoo"], {
    cwd: caseDirectory, env, detached: true, stdio: ["ignore", log, log],
  });
  fs.closeSync(log);
  let timedOut = false;
  let interrupted;
  let hardKill;
  const killGroup = (signal) => {
    if (!child.pid) return;
    try { process.kill(-child.pid, signal); } catch (error) {
      if (error.code !== "ESRCH") throw error;
    }
  };
  const terminate = () => {
    killGroup("SIGTERM");
    hardKill ??= setTimeout(() => killGroup("SIGKILL"), 2000);
  };
  const onInterrupt = () => { interrupted = "SIGINT"; terminate(); };
  const onTerminate = () => { interrupted = "SIGTERM"; terminate(); };
  process.once("SIGINT", onInterrupt);
  process.once("SIGTERM", onTerminate);
  const timeout = setTimeout(() => { timedOut = true; terminate(); }, 180_000);
  try {
    return await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code, signal) => resolve({
        milliseconds: Date.now() - started, code, signal, timedOut, interrupted,
        log: logPath, nativeLoadLog: auditPath,
      }));
    });
  } finally {
    clearTimeout(timeout);
    // A timeout/signal may leave descendants after their direct parent exits.
    if (hardKill) await sleep(2100);
    clearTimeout(hardKill);
    process.removeListener("SIGINT", onInterrupt);
    process.removeListener("SIGTERM", onTerminate);
  }
}

function statistics(values) {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = sorted.length / 2;
  return { runs: sorted.length, medianMs: (sorted[middle - 1] + sorted[middle]) / 2, minMs: sorted[0], maxMs: sorted.at(-1) };
}

async function main() {
  const benchmarkSha = command("git", ["rev-parse", "HEAD"]);
  assert.equal(benchmarkSha, "0dffa8f53bf349fab294d1d91add14cf882ebef8", "Unexpected benchmark source");
  const caseRequire = createRequire(path.join(caseDirectory, "package.json"));
  const packJson = caseRequire.resolve("@utoo/pack/package.json");
  const packRequire = createRequire(packJson);
  const platformJson = packRequire.resolve("@utoo/pack-darwin-arm64/package.json");
  const pack = readJson(packJson);
  const platform = readJson(platformJson);
  assert.equal(pack.version, "1.6.3");
  assert.equal(platform.version, "1.6.3");
  const variants = [
    { name: "published-z", path: path.resolve(path.dirname(platformJson), platform.main) },
    { name: "upstream-aligned", path: required("CANDIDATE_NATIVE") },
  ].map((native) => ({ ...native, path: fs.realpathSync(native.path), sha256: sha256(native.path), bytes: fs.statSync(native.path).size }));
  assert.notEqual(variants[0].sha256, variants[1].sha256, "Candidate equals published native");
  writeJson("native-inputs.json", {
    metadata: path.dirname(platformJson),
    baseline: variants[0].path,
    candidate: variants[1].path,
  });
  const metadata = {
    benchmarkSha, recordedAt: new Date().toISOString(), case: "react-dashboard",
    platform: process.platform, architecture: process.arch, node: process.version,
    cpuModel: os.cpus()[0]?.model, cpuCount: os.cpus().length, memoryBytes: os.totalmem(),
    osVersion: os.version(), osRelease: os.release(), swVers: command("sw_vers"),
    availableParallelism: os.availableParallelism(),
    turboTasksParallelism: process.env.TURBO_TASKS_AVAILABLE_PARALLELISM || "automatic",
    packVersion: pack.version, platformPackageVersion: platform.version,
    lockfileSha256: sha256(path.join(root, "pnpm-lock.yaml")),
    pnpmVersion: command("pnpm", ["--version"]),
    preloadSha256: sha256(preload), variants, timeoutMs: 180_000,
    timer: "Date.now before spawning node --run build:utoo until process exit",
    warmupRounds: 2, measuredRounds: 4, originalRunId: 43649700631,
    cacheCleanup: "Original cleanCache list at root and case; dist removed before every build",
  };
  writeJson("metadata.json", metadata);
  let referenceOutput;
  for (let round = 0; round < 6; round++) {
    const warmup = round < 2;
    const order = round % 2 === 0 ? variants : [...variants].reverse();
    for (const native of order) {
      for (const cache of ["cold", "warm"]) {
        if (cache === "cold") cleanCache();
        fs.rmSync(path.join(caseDirectory, "dist"), { recursive: true, force: true });
        await sleep(1000);
        const id = `${warmup ? "warmup" : "measured"}-${warmup ? round + 1 : round - 1}-${native.name}-${cache}`;
        console.log(`Starting ${id}`);
        const row = { id, round: warmup ? round + 1 : round - 1, warmup, variant: native.name, cache, nativeSha256: native.sha256, ...await build(native, id) };
        rows.push(row);
        writeJson("runs.json", rows);
        assert.equal(row.code, 0, `Build failed: ${id}; see ${row.log}`);
        assert(!row.timedOut && !row.interrupted, `Build interrupted: ${id}`);
        const loads = fs.readFileSync(row.nativeLoadLog, "utf8").trim().split("\n").filter(Boolean).map(JSON.parse);
        assert(loads.length > 0, `Selected native was not observed by dlopen: ${id}`);
        assert(loads.every((load) => load.event === "dlopen-success" && load.native === native.path && load.sha256 === native.sha256));
        row.nativeLoads = loads;
        row.output = outputSnapshot();
        referenceOutput ??= row.output;
        assert.deepEqual(row.output, referenceOutput, `Output differs: ${id}`);
        writeJson("runs.json", rows);
        console.log(`${id}: ${row.milliseconds}ms, ${row.output.files} files, ${row.output.bytes} bytes`);
      }
    }
  }
  const summary = { metadata, allOutputsIdentical: true, output: referenceOutput, variants: {} };
  for (const native of variants) {
    summary.variants[native.name] = Object.fromEntries(["cold", "warm"].map((cache) => [cache, statistics(rows.filter((row) => !row.warmup && row.variant === native.name && row.cache === cache).map((row) => row.milliseconds))]));
  }
  summary.coldReductionPercent = 100 * (1 - summary.variants["upstream-aligned"].cold.medianMs / summary.variants["published-z"].cold.medianMs);
  writeJson("summary.json", summary);
  const table = variants.map((native) => {
    const stats = summary.variants[native.name];
    const cell = (value) => `${(value.medianMs / 1000).toFixed(3)} (${(value.minMs / 1000).toFixed(3)}–${(value.maxMs / 1000).toFixed(3)})`;
    return `| ${native.name} | ${cell(stats.cold)} | ${cell(stats.warm)} | ${native.bytes} |`;
  });
  fs.writeFileSync(path.join(results, "summary.md"), [
    "# M1 native release comparison", "", "Times are process wall seconds: median (range), four measured runs per cell.", "",
    "| Native | Cold | Warm | Native bytes |", "| --- | ---: | ---: | ---: |", ...table, "",
    `Cold time reduction: ${summary.coldReductionPercent.toFixed(2)}%.`,
    `All ${rows.length} builds produced identical output: ${referenceOutput.files} files, ${referenceOutput.bytes} bytes, SHA-256 ${referenceOutput.sha256}.`,
    `Machine: ${metadata.cpuModel}; ${metadata.cpuCount} CPUs; ${metadata.memoryBytes} memory bytes; ${metadata.node}.`,
    `Benchmark source: ${benchmarkSha}. Automatic worker selection: ${metadata.turboTasksParallelism}.`, "",
  ].join("\n"));
  console.log(JSON.stringify(summary.variants, null, 2));
}

main().catch((error) => {
  writeJson("error.json", { error: error.stack, completedRuns: rows.length });
  console.error(error);
  process.exitCode = 1;
});
