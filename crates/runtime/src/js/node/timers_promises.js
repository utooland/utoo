// node:timers/promises module for utoo-runtime
import { promises } from "ext:utoo_rt_ext/node/timers";

const { setTimeout, setInterval, setImmediate } = promises;

export default promises;
export { setTimeout, setInterval, setImmediate };
