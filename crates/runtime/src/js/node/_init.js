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
import _v8 from "ext:utoo_rt_ext/node/v8";
import _cluster from "ext:utoo_rt_ext/node/cluster";
import _child_process from "ext:utoo_rt_ext/node/child_process";
import _tty from "ext:utoo_rt_ext/node/tty";
import _dns from "ext:utoo_rt_ext/node/dns";
import _dgram from "ext:utoo_rt_ext/node/dgram";
import _tls from "ext:utoo_rt_ext/node/tls";
import _worker_threads from "ext:utoo_rt_ext/node/worker_threads";
import _perf_hooks from "ext:utoo_rt_ext/node/perf_hooks";
import _module from "ext:utoo_rt_ext/node/module";
import _readline from "ext:utoo_rt_ext/node/readline";
import _diagnostics_channel from "ext:utoo_rt_ext/node/diagnostics_channel";
import _console from "ext:utoo_rt_ext/node/console";
import _timers from "ext:utoo_rt_ext/node/timers";
import _constants from "ext:utoo_rt_ext/node/constants";
import _domain from "ext:utoo_rt_ext/node/domain";
import _util_types from "ext:utoo_rt_ext/node/util_types";
import _stream_promises from "ext:utoo_rt_ext/node/stream_promises";
import _stream_web from "ext:utoo_rt_ext/node/stream_web";
import _stream_consumers from "ext:utoo_rt_ext/node/stream_consumers";
import _inspector from "ext:utoo_rt_ext/node/inspector";

// Make Buffer globally available (Node.js compat)
if (_buffer && _buffer.Buffer) {
  globalThis.Buffer = _buffer.Buffer;
}

// Make URL/URLSearchParams globally available (Web platform + Node.js compat)
if (!globalThis.URL && _url.URL) {
  globalThis.URL = _url.URL;
}
if (!globalThis.URLSearchParams && _url.URLSearchParams) {
  globalThis.URLSearchParams = _url.URLSearchParams;
}

// MessagePort / MessageChannel / BroadcastChannel from worker_threads
if (!globalThis.MessagePort) {
  globalThis.MessagePort = _worker_threads.MessagePort;
}
if (!globalThis.MessageChannel) {
  globalThis.MessageChannel = _worker_threads.MessageChannel;
}
if (!globalThis.BroadcastChannel) {
  globalThis.BroadcastChannel = _worker_threads.BroadcastChannel;
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
    ["v8", _v8],
    ["cluster", _cluster],
    ["child_process", _child_process],
    ["tty", _tty],
    ["dns", _dns],
    ["dgram", _dgram],
    ["tls", _tls],
    ["worker_threads", _worker_threads],
    ["perf_hooks", _perf_hooks],
    ["module", _module],
    ["readline", _readline],
    ["diagnostics_channel", _diagnostics_channel],
    ["console", _console],
    ["timers", _timers],
    ["timers/promises", _timers.promises || {}],
    ["constants", _constants],
    ["domain", _domain],
    ["util/types", _util_types],
    ["stream/promises", _stream_promises],
    ["stream/web", _stream_web],
    ["stream/consumers", _stream_consumers],
    ["inspector", _inspector],
  ]) {
    b.set(name, mod);
    b.set("node:" + name, mod);
  }
  b.set("fs/promises", _fsp);
  b.set("node:fs/promises", _fsp);
}
