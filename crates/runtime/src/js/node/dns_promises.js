import dns from "ext:utoo_rt_ext/node/dns";

const { promises } = dns;
const dnsPromises = { ...promises };
dnsPromises.default = dnsPromises;

export default dnsPromises;
export const { lookup, resolve } = promises;
