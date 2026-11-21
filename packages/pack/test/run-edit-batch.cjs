const { spawn } = require("node:child_process");
const path = require("node:path");

// Rust panics in background threads do not necessarily fail node:test. Preserve
// natural child-process shutdown and reject these diagnostics as test failures.
const child = spawn(
  process.execPath,
  [
    "--test",
    ...process.argv.slice(2),
    path.join(__dirname, "editBatch.native.cjs"),
  ],
  { stdio: ["inherit", "pipe", "pipe"], env: process.env },
);
let nativeFailure = false;
for (const [stream, destination] of [
  [child.stdout, process.stdout],
  [child.stderr, process.stderr],
]) {
  let tail = "";
  stream.on("data", (chunk) => {
    destination.write(chunk);
    const text = tail + chunk.toString();
    if (/panicked at|TurbopackInternalError/.test(text)) nativeFailure = true;
    tail = text.slice(-8192);
  });
}
child.on("error", (error) => {
  console.error(error);
  process.exitCode = 1;
});
child.on("close", (code, signal) => {
  if (nativeFailure)
    console.error(
      "Native edit-batch regression emitted a panic or internal error",
    );
  if (signal)
    console.error(`Native edit-batch regression terminated by ${signal}`);
  process.exitCode = code === 0 && !nativeFailure ? 0 : 1;
});
