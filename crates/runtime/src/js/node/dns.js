// Minimal dns module stub for utoo-runtime

function lookup(hostname, opts, cb) {
  if (typeof opts === "function") { cb = opts; opts = {}; }
  // Resolve localhost
  if (hostname === "localhost" || hostname === "127.0.0.1") {
    if (cb) cb(null, "127.0.0.1", 4);
    return;
  }
  if (cb) cb(new Error(`dns.lookup() is not fully supported in utoo-runtime: ${hostname}`));
}

function resolve(hostname, rrtype, cb) {
  if (typeof rrtype === "function") { cb = rrtype; }
  if (cb) cb(new Error("dns.resolve() is not supported in utoo-runtime"));
}

function resolve4(hostname, opts, cb) {
  if (typeof opts === "function") { cb = opts; }
  if (cb) cb(new Error("dns.resolve4() is not supported in utoo-runtime"));
}

function resolve6(hostname, opts, cb) {
  if (typeof opts === "function") { cb = opts; }
  if (cb) cb(new Error("dns.resolve6() is not supported in utoo-runtime"));
}

function reverse(ip, cb) {
  if (cb) cb(new Error("dns.reverse() is not supported in utoo-runtime"));
}

function setServers() {}
function getServers() { return []; }

const promises = {
  lookup(hostname) {
    return new Promise((resolve, reject) => {
      lookup(hostname, (err, addr, family) => {
        if (err) reject(err);
        else resolve({ address: addr, family });
      });
    });
  },
  resolve() { return Promise.reject(new Error("dns.promises.resolve() is not supported")); },
};

const dns = {
  lookup, resolve, resolve4, resolve6, reverse,
  setServers, getServers, promises,
  ADDRCONFIG: 0, V4MAPPED: 0, ALL: 0,
};
dns.default = dns;

export default dns;
export { lookup, resolve, resolve4, resolve6, reverse, setServers, getServers, promises };
