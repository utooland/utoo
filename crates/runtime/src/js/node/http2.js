// Stub: node:http2 - not yet implemented
const http2 = {
  createServer() { throw new Error("http2.createServer is not implemented"); },
  createSecureServer() { throw new Error("http2.createSecureServer is not implemented"); },
  connect() { throw new Error("http2.connect is not implemented"); },
  constants: {},
  getDefaultSettings() { return {}; },
  getPackedSettings() { return Buffer.alloc(0); },
  getUnpackedSettings() { return {}; },
  sensitiveHeaders: Symbol("nodejs.http2.sensitiveHeaders"),
};
export default http2;
export const { createServer, createSecureServer, connect, constants, getDefaultSettings, getPackedSettings, getUnpackedSettings, sensitiveHeaders } = http2;
