const { spawn } = require("node:child_process");
const path = require("node:path");

const packagePath = require.resolve("@napi-rs/cli/package.json");
const { bin } = require(packagePath);
const cli = path.resolve(path.dirname(packagePath), bin.napi);
const config = path.resolve(__dirname, "../../../.cargo/pack-release.toml");
const args = process.argv.slice(2);

// Keep appended --target and cross-compilation options in NAPI's argument list.
// Put defaults before explicit Cargo options so callers can override them.
const separator = args.indexOf("--");
if (separator === -1) args.push("--", "--config", config);
else args.splice(separator + 1, 0, "--config", config);

const child = spawn(process.execPath, [cli, "build", ...args], {
  stdio: "inherit",
  env: process.env,
});

const forwardInterrupt = () => child.kill("SIGINT");
const forwardTerminate = () => child.kill("SIGTERM");
process.on("SIGINT", forwardInterrupt);
process.on("SIGTERM", forwardTerminate);

child.once("error", (error) => {
  console.error(error.message);
  process.exitCode = 1;
});
child.once("exit", (code, signal) => {
  process.removeListener("SIGINT", forwardInterrupt);
  process.removeListener("SIGTERM", forwardTerminate);
  if (signal) process.kill(process.pid, signal);
  else process.exitCode = code ?? 1;
});
