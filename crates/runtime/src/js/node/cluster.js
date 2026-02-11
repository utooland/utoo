// Minimal cluster module stub for utoo-runtime
import EventEmitter from "ext:utoo_rt_ext/node/events";

class Cluster extends EventEmitter {
  constructor() {
    super();
    this.isMaster = true;
    this.isPrimary = true;
    this.isWorker = false;
    this.workers = {};
    this.settings = {};
    this.SCHED_NONE = 1;
    this.SCHED_RR = 2;
    this.schedulingPolicy = 2;
  }

  setupPrimary() {}
  setupMaster() {}
  fork() {
    throw new Error("cluster.fork() is not supported in utoo-runtime");
  }
  disconnect(cb) { if (cb) cb(); }
}

const cluster = new Cluster();
cluster.default = cluster;

export default cluster;
export const isMaster = cluster.isMaster;
export const isPrimary = cluster.isPrimary;
export const isWorker = cluster.isWorker;
export const workers = cluster.workers;
export const fork = cluster.fork.bind(cluster);
export const disconnect = cluster.disconnect.bind(cluster);
export const setupPrimary = cluster.setupPrimary.bind(cluster);
export const setupMaster = cluster.setupMaster.bind(cluster);
