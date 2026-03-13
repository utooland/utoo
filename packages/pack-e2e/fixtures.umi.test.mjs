import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { pathToFileURL } from "node:url";

const packE2eRoot = import.meta.dirname;
const fixturesRoot = path.join(packE2eRoot, "fixtures.umi");

function findUmiBin() {
  const candidates = [];

  const localBin = path.join(packE2eRoot, "node_modules", ".bin");
  const rootBin = path.join(packE2eRoot, "..", "..", "node_modules", ".bin");
  candidates.push(path.join(localBin, "umi"), path.join(localBin, "umi.cmd"));
  candidates.push(path.join(rootBin, "umi"), path.join(rootBin, "umi.cmd"));

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }

  return null;
}

function parseBuildResult(distDir) {
  const files = {};

  function walk(dir) {
    for (const fileName of fs.readdirSync(dir)) {
      const filePath = path.join(dir, fileName);
      const stat = fs.statSync(filePath);

      if (stat.isDirectory()) {
        walk(filePath);
      } else {
        const relPath = path.relative(distDir, filePath).split(path.sep).join("/");
        files[relPath] = fs.readFileSync(filePath, "utf8");
      }
    }
  }

  walk(distDir);
  return { distDir, files };
}

function getFixtureCases() {
  if (!fs.existsSync(fixturesRoot)) {
    return [];
  }

  return fs.readdirSync(fixturesRoot).filter((dirName) => {
    const fixtureDir = path.join(fixturesRoot, dirName);
    if (!fs.statSync(fixtureDir).isDirectory()) {
      return false;
    }

    return (
      fs.existsSync(path.join(fixtureDir, "expect.js")) &&
      fs.existsSync(path.join(fixtureDir, ".umirc.ts"))
    );
  });
}

async function runUmiBuild(cwd, umiBin) {
  await new Promise((resolve, reject) => {
    const child = spawn(umiBin, ["build"], {
      cwd,
      stdio: "inherit",
      shell: false,
      env: { ...process.env, COMPRESS: "none" },
    });

    child.on("exit", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`umi build failed with exit code ${code}`));
      }
    });
    child.on("error", reject);
  });
}

const fixtureCases = getFixtureCases();
assert.ok(fixtureCases.length > 0, "No fixture cases found in fixtures.umi");
const umiBin = findUmiBin();

for (const caseName of fixtureCases) {
  const testFn = umiBin ? test : test.skip;

  testFn(caseName, async () => {
    const fixtureDir = path.join(fixturesRoot, caseName);
    const distDir = path.join(fixtureDir, "dist");
    const expectPath = path.join(fixtureDir, "expect.js");

    fs.rmSync(distDir, { recursive: true, force: true });

    await runUmiBuild(fixtureDir, umiBin);

    assert.ok(fs.existsSync(distDir), `${caseName}: dist directory not generated`);
    const buildResult = parseBuildResult(distDir);

    const expectModule = await import(pathToFileURL(expectPath).href);
    const runExpect = expectModule.default;
    assert.equal(typeof runExpect, "function", `${caseName}: expect.js must export a function`);

    await runExpect(buildResult);
  });
}
