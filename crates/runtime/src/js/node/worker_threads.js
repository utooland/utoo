// Minimal worker_threads module stub for utoo-runtime
import EventEmitter from "ext:utoo_rt_ext/node/events";

const isMainThread = true;
const parentPort = null;
const workerData = undefined;
const threadId = 0;
const resourceLimits = {};

class Worker extends EventEmitter {
  constructor() {
    super();
    throw new Error("worker_threads.Worker is not supported in utoo-runtime");
  }
}

class MessageChannel {
  constructor() {
    this.port1 = new MessagePort();
    this.port2 = new MessagePort();
  }
}

class MessagePort extends EventEmitter {
  constructor() { super(); }
  postMessage() {}
  start() {}
  close() {}
  ref() { return this; }
  unref() { return this; }
}

class BroadcastChannel extends EventEmitter {
  constructor(name) { super(); this.name = name; }
  postMessage() {}
  close() {}
  ref() { return this; }
  unref() { return this; }
}

function getEnvironmentData() { return undefined; }
function setEnvironmentData() {}
function markAsUntransferable() {}
function moveMessagePortToContext() { throw new Error("Not supported"); }
function receiveMessageOnPort() { return undefined; }

const worker_threads = {
  isMainThread, parentPort, workerData, threadId, resourceLimits,
  Worker, MessageChannel, MessagePort, BroadcastChannel,
  getEnvironmentData, setEnvironmentData, markAsUntransferable,
  moveMessagePortToContext, receiveMessageOnPort,
};
worker_threads.default = worker_threads;

export default worker_threads;
export {
  isMainThread, parentPort, workerData, threadId, resourceLimits,
  Worker, MessageChannel, MessagePort, BroadcastChannel,
  getEnvironmentData, setEnvironmentData, markAsUntransferable,
  moveMessagePortToContext, receiveMessageOnPort,
};
