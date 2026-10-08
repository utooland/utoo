export function getCache() {
  return require.cache;
}

export function counterId() {
  return require.resolve("./counter.cjs");
}

export function futureId() {
  return require.resolve("./future.cjs");
}

export function readCounter() {
  return require("./counter.cjs");
}

export function readFuture() {
  return require("./future.cjs");
}
