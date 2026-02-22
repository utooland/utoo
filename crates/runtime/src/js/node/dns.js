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

class Resolver {
  constructor(opts) {
    this._servers = [];
  }
  setLocalAddress() {}
  cancel() {}
  getServers() { return this._servers; }
  setServers(s) { this._servers = s || []; }
  resolve(hostname, rrtype) {
    return Promise.reject(new Error("dns.Resolver.resolve() is not fully supported: " + hostname));
  }
  resolve4(hostname, opts) {
    if (hostname === "localhost" || hostname === "127.0.0.1") {
      return Promise.resolve(opts && opts.ttl ? [{ address: "127.0.0.1", ttl: 60 }] : ["127.0.0.1"]);
    }
    return Promise.reject(new Error("dns.Resolver.resolve4() is not fully supported: " + hostname));
  }
  resolve6(hostname) {
    return Promise.reject(new Error("dns.Resolver.resolve6() is not fully supported: " + hostname));
  }
  resolveSrv(hostname) {
    return Promise.resolve([]);
  }
  resolveTxt(hostname) {
    return Promise.resolve([]);
  }
  resolveCname(hostname) {
    return Promise.resolve([]);
  }
  resolveAny(hostname) {
    return Promise.resolve([]);
  }
  reverse(ip) {
    return Promise.resolve([]);
  }
}

// Callback-style Resolver
class CallbackResolver {
  constructor(opts) {
    this._inner = new Resolver(opts);
  }
  setLocalAddress() {}
  cancel() {}
  getServers() { return this._inner.getServers(); }
  setServers(s) { this._inner.setServers(s); }
  resolve(hostname, rrtype, cb) {
    if (typeof rrtype === "function") { cb = rrtype; }
    this._inner.resolve(hostname).then(r => cb(null, r)).catch(e => cb(e));
  }
  resolve4(hostname, opts, cb) {
    if (typeof opts === "function") { cb = opts; opts = {}; }
    this._inner.resolve4(hostname, opts).then(r => cb(null, r)).catch(e => cb(e));
  }
  resolve6(hostname, opts, cb) {
    if (typeof opts === "function") { cb = opts; }
    this._inner.resolve6(hostname).then(r => cb(null, r)).catch(e => cb(e));
  }
  resolveSrv(hostname, cb) {
    this._inner.resolveSrv(hostname).then(r => cb(null, r)).catch(e => cb(e));
  }
  resolveTxt(hostname, cb) {
    this._inner.resolveTxt(hostname).then(r => cb(null, r)).catch(e => cb(e));
  }
}

const promises = {
  Resolver,
  lookup(hostname) {
    return new Promise((resolve, reject) => {
      lookup(hostname, (err, addr, family) => {
        if (err) reject(err);
        else resolve({ address: addr, family });
      });
    });
  },
  resolve() { return Promise.reject(new Error("dns.promises.resolve() is not supported")); },
  resolve4(hostname, opts) {
    var r = new Resolver();
    return r.resolve4(hostname, opts);
  },
  resolveSrv(hostname) {
    var r = new Resolver();
    return r.resolveSrv(hostname);
  },
};

const dns = {
  lookup, resolve, resolve4, resolve6, reverse,
  setServers, getServers, promises,
  Resolver: CallbackResolver,
  ADDRCONFIG: 0, V4MAPPED: 0, ALL: 0,
  NODATA: "ENODATA", FORMERR: "EFORMERR", SERVFAIL: "ESERVFAIL",
  NOTFOUND: "ENOTFOUND", NOTIMP: "ENOTIMP", REFUSED: "EREFUSED",
  BADQUERY: "EBADQUERY", BADNAME: "EBADNAME", BADFAMILY: "EBADFAMILY",
  BADRESP: "EBADRESP", CONNREFUSED: "ECONNREFUSED", TIMEOUT: "ETIMEOUT",
  EOF: "EOF", FILE: "EFILE", NOMEM: "ENOMEM", DESTRUCTION: "EDESTRUCTION",
  BADSTR: "EBADSTR", BADFLAGS: "EBADFLAGS", NONAME: "ENONAME",
  BADHINTS: "EBADHINTS", NOTINITIALIZED: "ENOTINITIALIZED",
  LOADIPHLPAPI: "ELOADIPHLPAPI", ADDRGETNETWORKPARAMS: "EADDRGETNETWORKPARAMS",
  CANCELLED: "ECANCELLED",
};
dns.default = dns;

export default dns;
export { lookup, resolve, resolve4, resolve6, reverse, setServers, getServers, promises, CallbackResolver as Resolver };
