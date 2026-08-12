"use strict";

const binding = require(process.env.BINDING);
const lockfile = process.env.LOCKFILE;
const lock = binding.lockfileTryAcquireSync(lockfile, "musl smoke test");

if (!lock) {
  throw new Error(`failed to acquire ${lockfile}`);
}

binding.lockfileUnlockSync(lock);
console.log(`loaded ${process.env.BINDING}, acquired and released a lockfile`);
