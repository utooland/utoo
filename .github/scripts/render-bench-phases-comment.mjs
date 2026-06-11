#!/usr/bin/env node
// Render the pm-bench-phases sticky PR comment from the raw per-cell JSON
// exported by bench/pm-bench-phases.sh (BENCH_OUT_DIR/results-<registry>/),
// instead of awk-scraping fixed-width log tables.
//
// The headline is the verdict: utoo (PR build) vs utoo-next (baseline) per
// phase, with a significance gate so registry weather doesn't read as a
// regression. Raw numbers stay available below; resources fold into
// <details>.
//
// Usage: render-bench-phases-comment.mjs <platform> <os>
// Env:   PROJECT (default ant-design), OUT_DIR (default /tmp/pm-bench-output),
//        GITHUB_SHA / GITHUB_REPOSITORY / GITHUB_SERVER_URL / GITHUB_RUN_ID,
//        GITHUB_STEP_SUMMARY (appended when set).

import fs from "node:fs";
import path from "node:path";

const [platform = "linux", os = "ubuntu-latest"] = process.argv.slice(2);
const PROJECT = process.env.PROJECT || "ant-design";
const OUT_DIR = process.env.OUT_DIR || "/tmp/pm-bench-output";

const PHASES = [
  ["p0_full_cold", "p0 · full cold install"],
  ["p1_resolve", "p1 · resolve"],
  ["p3_cold_install", "p3 · cold install"],
  ["p4_warm_link", "p4 · warm link"],
];
const PM_ORDER = ["utoo", "utoo-alt", "utoo-next", "utoo-npm", "bun"];
const PM_LABEL = {
  utoo: "utoo (PR)",
  "utoo-alt": "utoo-alt (PR + alt env)",
  "utoo-next": "utoo-next (baseline)",
  "utoo-npm": "utoo-npm (published)",
  bun: "bun",
};

// Relative delta below this is never flagged, whatever the sigmas say.
const FLAG_THRESHOLD = 0.05;

const parseKey = (file, suffix) => {
  const base = file.slice(0, -suffix.length);
  for (const [phase] of PHASES) {
    const idx = base.indexOf(`_${phase}_`);
    if (idx === -1) continue;
    return { phase, pm: base.slice(idx + phase.length + 2) };
  }
  return null;
};

const readJson = (p) => {
  try {
    return JSON.parse(fs.readFileSync(p, "utf8"));
  } catch {
    return null;
  }
};

// registry label -> phase -> pm -> cell
const registries = new Map();
const cell = (reg, phase, pm) => {
  if (!registries.has(reg)) registries.set(reg, {});
  const phases = registries.get(reg);
  return ((phases[phase] ??= {})[pm] ??= {});
};

const resultDirs = fs.existsSync(OUT_DIR)
  ? fs
      .readdirSync(OUT_DIR)
      .filter((d) => d.startsWith("results-"))
      .map((d) => ({ reg: d.slice("results-".length), dir: path.join(OUT_DIR, d) }))
  : [];

for (const { reg, dir } of resultDirs) {
  for (const f of fs.readdirSync(dir)) {
    if (!f.startsWith(`${PROJECT}_`)) continue;
    const full = path.join(dir, f);
    if (f.endsWith("_failed.json")) {
      const key = parseKey(f, "_failed.json");
      if (key) cell(reg, key.phase, key.pm).failed = readJson(full)?.failed || "failed";
    } else if (f.endsWith("_metrics.jsonl")) {
      const key = parseKey(f, "_metrics.jsonl");
      if (!key) continue;
      const rows = fs
        .readFileSync(full, "utf8")
        .split("\n")
        .filter(Boolean)
        .map((l) => {
          try {
            return JSON.parse(l);
          } catch {
            return null;
          }
        })
        .filter(Boolean);
      if (!rows.length) continue;
      const avg = {};
      for (const k of Object.keys(rows[0])) {
        avg[k] = rows.reduce((s, r) => s + Number(r[k] || 0), 0) / rows.length;
      }
      cell(reg, key.phase, key.pm).metrics = avg;
    } else if (f.endsWith("_footprint.json")) {
      const key = parseKey(f, "_footprint.json");
      if (key) cell(reg, key.phase, key.pm).footprint = readJson(full);
    } else if (f.endsWith(".json")) {
      const key = parseKey(f, ".json");
      if (!key) continue;
      const r = readJson(full)?.results?.[0];
      if (!r) continue;
      cell(reg, key.phase, key.pm).timing = {
        mean: r.mean,
        stddev: r.stddev ?? 0,
        min: r.min,
        max: r.max,
        runs: r.times?.length ?? 0,
        times: r.times ?? [],
      };
    }
  }
}

const fmtS = (s) => (s >= 10 ? s.toFixed(1) : s.toFixed(2)) + "s";
const fmtB = (b) =>
  b >= 1 << 30
    ? (b / (1 << 30)).toFixed(2) + "G"
    : b >= 1 << 20
      ? (b / (1 << 20)).toFixed(0) + "M"
      : b >= 1 << 10
        ? (b / (1 << 10)).toFixed(0) + "K"
        : Math.round(b) + "B";
const fmtPct = (d) => (d > 0 ? "+" : "") + (d * 100).toFixed(1) + "%";

// Preferred gate: PAIRED per-round deltas. The interleaved bench runs each
// PM once per round, so times[i] across PMs share the same weather window —
// the median paired delta cancels drift that a means comparison can't. A
// null test (identical binaries) under the old per-PM-block shape read
// "p3 -27.9%" off one weather spike; pairing is what makes verdicts mean
// something. Flag only when the median is past the threshold AND ≥80% of
// rounds agree on the sign.
//
// Fallback (legacy non-interleaved data): Welch-style 2-sigma gate on the
// difference of means.
function compare(utoo, next) {
  if (!utoo?.timing || !next?.timing) return null;
  const a = utoo.timing;
  const b = next.timing;
  if (!b.mean) return null;

  if (a.times.length >= 3 && a.times.length === b.times.length) {
    const deltas = a.times.map((t, i) => (t - b.times[i]) / b.times[i]);
    const sorted = [...deltas].sort((x, y) => x - y);
    const mid = sorted.length >> 1;
    const median = sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
    const agreeing = deltas.filter((d) => Math.sign(d) === Math.sign(median)).length;
    const significant =
      Math.abs(median) > FLAG_THRESHOLD && agreeing >= Math.ceil(deltas.length * 0.8);
    const verdict = !significant ? "✅" : median < 0 ? "🚀" : "⚠️";
    return { delta: median, significant, verdict, paired: true };
  }

  const delta = (a.mean - b.mean) / b.mean;
  const se = Math.sqrt(
    (a.stddev * a.stddev) / Math.max(a.runs, 1) + (b.stddev * b.stddev) / Math.max(b.runs, 1),
  );
  const significant = Math.abs(delta) > FLAG_THRESHOLD && Math.abs(a.mean - b.mean) > 2 * se;
  const verdict = !significant ? "✅" : delta < 0 ? "🚀" : "⚠️";
  return { delta, significant, verdict, paired: false };
}

const sha = (process.env.GITHUB_SHA || "").slice(0, 7);
const runUrl = `${process.env.GITHUB_SERVER_URL || "https://github.com"}/${process.env.GITHUB_REPOSITORY || ""}/actions/runs/${process.env.GITHUB_RUN_ID || ""}`;

const lines = [];
lines.push(`## 📊 pm-bench-phases · \`${sha}\` · ${platform} (\`${os}\`)`);
lines.push("");
lines.push(`[Workflow run](${runUrl}) — ${PROJECT}`);
lines.push("");

if (!resultDirs.length) {
  lines.push("_No bench results captured (bench aborted before export?). See the workflow log._");
} else {
  for (const { reg } of resultDirs) {
    const phases = registries.get(reg) || {};

    // Headline: utoo vs utoo-next per phase.
    const verdicts = [];
    for (const [phase, label] of PHASES) {
      const pms = phases[phase];
      if (!pms) continue;
      const cmp = compare(pms["utoo"], pms["utoo-next"]);
      if (cmp) verdicts.push(`${cmp.verdict} ${label.split(" · ")[0]} ${fmtPct(cmp.delta)}`);
    }
    lines.push(`### ${reg}`);
    lines.push("");
    if (verdicts.length) {
      lines.push(`**utoo (PR) vs utoo-next (baseline):** ${verdicts.join(" · ")}`);
      lines.push("");
      lines.push(
        "_✅ within noise · 🚀 faster · ⚠️ slower — Δ is the median of per-round paired deltas (interleaved rounds share weather windows); flagged when |Δ| > 5% and ≥80% of rounds agree on sign_",
      );
      lines.push("");
    }

    for (const [phase, label] of PHASES) {
      const pms = phases[phase];
      if (!pms) continue;
      const present = PM_ORDER.filter((pm) => pms[pm]);
      if (!present.length) continue;

      lines.push(`#### ${label}`);
      lines.push("");
      lines.push("| PM | wall (mean ± σ) | min | user | sys | RSS | Δ vs baseline |");
      lines.push("|---|---|---|---|---|---|---|");
      for (const pm of present) {
        const c = pms[pm];
        if (c.failed) {
          lines.push(`| ${PM_LABEL[pm] ?? pm} | — ${c.failed} failed | | | | | |`);
          continue;
        }
        const t = c.timing;
        const m = c.metrics || {};
        const cmp = pm === "utoo-next" ? null : compare(c, pms["utoo-next"]);
        lines.push(
          `| ${PM_LABEL[pm] ?? pm} | ${t ? `${fmtS(t.mean)} ± ${fmtS(t.stddev)}` : "—"} | ${t ? fmtS(t.min) : "—"} | ${m.user_s != null ? fmtS(m.user_s) : "—"} | ${m.sys_s != null ? fmtS(m.sys_s) : "—"} | ${m.rss ? fmtB(m.rss) : "—"} | ${cmp ? `${fmtPct(cmp.delta)} ${cmp.verdict}` : "—"} |`,
        );
      }
      lines.push("");
    }

    // Resources fold.
    lines.push("<details><summary>Resources & footprint</summary>");
    lines.push("");
    for (const [phase, label] of PHASES) {
      const pms = phases[phase];
      if (!pms) continue;
      const present = PM_ORDER.filter((pm) => pms[pm] && !pms[pm].failed);
      if (!present.length) continue;
      lines.push(`**${label}**`);
      lines.push("");
      lines.push("| PM | vCtx | iCtx | netRX | netTX | cache | node_modules | lock |");
      lines.push("|---|---|---|---|---|---|---|---|");
      for (const pm of present) {
        const m = pms[pm].metrics || {};
        const fp = pms[pm].footprint || {};
        lines.push(
          `| ${pm} | ${Math.round(m.vol_ctx || 0)} | ${Math.round(m.invol_ctx || 0)} | ${fmtB(m.net_rx || 0)} | ${fmtB(m.net_tx || 0)} | ${fmtB(fp.cache || 0)} | ${fmtB(fp.node_modules || 0)} | ${fmtB(fp.lockfile || 0)} |`,
        );
      }
      lines.push("");
    }
    lines.push("</details>");
    lines.push("");
  }
}

const body = lines.join("\n");
fs.mkdirSync(OUT_DIR, { recursive: true });
fs.writeFileSync(path.join(OUT_DIR, "pr_comment.md"), body);
if (process.env.GITHUB_STEP_SUMMARY) {
  fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, body + "\n");
}
console.log(body);
