// Minimal perf_hooks module stub for utoo-runtime

class PerformanceObserver {
  constructor(cb) { this._cb = cb; }
  observe() {}
  disconnect() {}
  takeRecords() { return []; }
}

class PerformanceEntry {
  constructor(name, type, startTime, duration) {
    this.name = name || "";
    this.entryType = type || "";
    this.startTime = startTime || 0;
    this.duration = duration || 0;
  }
  toJSON() {
    return { name: this.name, entryType: this.entryType, startTime: this.startTime, duration: this.duration };
  }
}

const performance = {
  now() { return Date.now(); },
  timeOrigin: Date.now(),
  mark() {},
  measure() {},
  clearMarks() {},
  clearMeasures() {},
  getEntries() { return []; },
  getEntriesByName() { return []; },
  getEntriesByType() { return []; },
  toJSON() { return { timeOrigin: this.timeOrigin }; },
  nodeTiming: {
    name: "node",
    entryType: "node",
    startTime: 0,
    duration: 0,
    nodeStart: 0,
    v8Start: 0,
    bootstrapComplete: 0,
    environment: 0,
    loopStart: 0,
    loopExit: 0,
    idleTime: 0,
  },
};

function monitorEventLoopDelay() {
  return {
    enable() {},
    disable() {},
    reset() {},
    percentile() { return 0; },
    min: 0, max: 0, mean: 0, stddev: 0,
    percentiles: new Map(),
  };
}

const constants = {};

const perf_hooks = {
  PerformanceObserver, PerformanceEntry, performance,
  monitorEventLoopDelay, constants,
};
perf_hooks.default = perf_hooks;

export default perf_hooks;
export { PerformanceObserver, PerformanceEntry, performance, monitorEventLoopDelay, constants };
