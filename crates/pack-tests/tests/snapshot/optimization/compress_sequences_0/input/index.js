function sideEffect(v) {
  globalThis.__seq_log = (globalThis.__seq_log || 0) + v;
  return globalThis.__seq_log;
}

export function calc(a, b) {
  const first = sideEffect(a);
  const second = sideEffect(b);
  return first + second;
}

calc(1, 2);
