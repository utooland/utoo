// This module ensures all node: compatibility modules are evaluated during
// runtime initialization so they're available for user imports.
// It also populates __cjs_builtins for require() support.
import _fs from "ext:utoo_rt_ext/node/fs";
import _fsp from "ext:utoo_rt_ext/node/fs_promises";
import _path from "ext:utoo_rt_ext/node/path";
import _os from "ext:utoo_rt_ext/node/os";
import _url from "ext:utoo_rt_ext/node/url";
import _buffer from "ext:utoo_rt_ext/node/buffer";
import _events from "ext:utoo_rt_ext/node/events";
import _util from "ext:utoo_rt_ext/node/util";
import _assert from "ext:utoo_rt_ext/node/assert";
import _querystring from "ext:utoo_rt_ext/node/querystring";
import _string_decoder from "ext:utoo_rt_ext/node/string_decoder";
import _stream from "ext:utoo_rt_ext/node/stream";
import _net from "ext:utoo_rt_ext/node/net";
import _http from "ext:utoo_rt_ext/node/http";
import _https from "ext:utoo_rt_ext/node/https";
import _async_hooks from "ext:utoo_rt_ext/node/async_hooks";
import _crypto from "ext:utoo_rt_ext/node/crypto";
import _zlib from "ext:utoo_rt_ext/node/zlib";

// Make Buffer globally available (Node.js compat)
if (_buffer && _buffer.Buffer) {
  globalThis.Buffer = _buffer.Buffer;
}

const b = globalThis.__cjs_builtins;
if (b) {
  for (const [name, mod] of [
    ["fs", _fs],
    ["path", _path],
    ["os", _os],
    ["url", _url],
    ["buffer", _buffer],
    ["events", _events],
    ["util", _util],
    ["assert", _assert],
    ["querystring", _querystring],
    ["string_decoder", _string_decoder],
    ["stream", _stream],
    ["net", _net],
    ["http", _http],
    ["https", _https],
    ["async_hooks", _async_hooks],
    ["crypto", _crypto],
    ["zlib", _zlib],
  ]) {
    b.set(name, mod);
    b.set("node:" + name, mod);
  }
  b.set("fs/promises", _fsp);
  b.set("node:fs/promises", _fsp);
}
