const fs = require("fs");
const path = require("path");

const packageJson = require("../package.json");
const expectedVersion = process.argv[2] ?? packageJson.version;
const bindingPath = path.join(__dirname, "../src/binding.js");
const bindingSource = fs.readFileSync(bindingPath, "utf8");
const versions = [
  ...bindingSource.matchAll(/bindingPackageVersion !== ["']([^"']+)["']/g),
].map((match) => match[1]);

if (versions.length === 0) {
  throw new Error(`No native binding package version checks found in ${bindingPath}`);
}

const mismatchedVersions = [...new Set(versions.filter((version) => version !== expectedVersion))];

if (mismatchedVersions.length > 0) {
  throw new Error(
    `Expected ${expectedVersion} in ${bindingPath}, found ${mismatchedVersions.join(", ")}`,
  );
}

console.log(`Verified ${versions.length} native binding version checks for ${expectedVersion}`);
