import EventEmitter from "ext:utoo_rt_ext/node/events";
import { Readable, Writable } from "ext:utoo_rt_ext/node/stream";
import net from "ext:utoo_rt_ext/node/net";

class IncomingMessage extends Readable {
  constructor(socket) {
    super();
    this.socket = socket;
    this.connection = socket;
    this.httpVersion = "1.1";
    this.httpVersionMajor = 1;
    this.httpVersionMinor = 1;
    this.headers = {};
    this.rawHeaders = [];
    this.method = null;
    this.url = null;
    this.statusCode = null;
    this.statusMessage = null;
    this.complete = false;
    this.aborted = false;
  }

  setTimeout(ms, cb) {
    if (cb) this.once("timeout", cb);
    return this;
  }

  destroy(err) {
    this.socket.destroy(err);
    return super.destroy(err);
  }
}

class ServerResponse extends Writable {
  constructor(req) {
    super();
    this.socket = req.socket;
    this.connection = req.socket;
    this._req = req;
    this.statusCode = 200;
    this.statusMessage = "OK";
    this._headers = {};
    this._headersSent = false;
    this._body = [];
    this.finished = false;
    this.writableEnded = false;
    this.writableFinished = false;
  }

  get headersSent() {
    return this._headersSent;
  }

  setHeader(name, value) {
    this._headers[name.toLowerCase()] = value;
    return this;
  }

  getHeader(name) {
    return this._headers[name.toLowerCase()];
  }

  getHeaders() {
    return { ...this._headers };
  }

  getHeaderNames() {
    return Object.keys(this._headers);
  }

  hasHeader(name) {
    return name.toLowerCase() in this._headers;
  }

  removeHeader(name) {
    delete this._headers[name.toLowerCase()];
  }

  writeHead(statusCode, statusMessage, headers) {
    if (typeof statusMessage === "object") {
      headers = statusMessage;
      statusMessage = undefined;
    }
    this.statusCode = statusCode;
    if (statusMessage) this.statusMessage = statusMessage;
    if (headers) {
      for (const [k, v] of Object.entries(headers)) this.setHeader(k, v);
    }
    this._sendHeaders();
    return this;
  }

  get _header() {
    // Used by some middleware to check if headers have been composed
    return this._headersSent ? "sent" : null;
  }

  _implicitHeader() {
    if (!this._headersSent) {
      this.writeHead(this.statusCode);
    }
  }

  flushHeaders() {
    this._sendHeaders();
  }

  _sendHeaders() {
    if (this._headersSent) return;
    this._headersSent = true;

    // If no content-length and no transfer-encoding, signal connection close
    if (!this.hasHeader("content-length") && !this.hasHeader("transfer-encoding")) {
      if (!this.hasHeader("connection")) {
        this.setHeader("connection", "close");
      }
    }

    let head = `HTTP/1.1 ${this.statusCode} ${this.statusMessage}\r\n`;
    for (const [k, v] of Object.entries(this._headers)) {
      if (Array.isArray(v)) {
        for (const item of v) head += `${k}: ${item}\r\n`;
      } else {
        head += `${k}: ${v}\r\n`;
      }
    }
    head += "\r\n";

    this.socket.write(new TextEncoder().encode(head));
  }

  _write(chunk, encoding, cb) {
    this._sendHeaders();
    if (typeof chunk === "string") chunk = new TextEncoder().encode(chunk);
    this.socket.write(chunk, encoding, cb);
  }

  write(chunk, encoding, cb) {
    if (typeof encoding === "function") { cb = encoding; encoding = "utf8"; }
    this._sendHeaders();
    if (typeof chunk === "string") chunk = new TextEncoder().encode(chunk);
    this.socket.write(chunk);
    if (cb) cb();
    return true;
  }

  end(data, encoding, cb) {
    if (typeof data === "function") { cb = data; data = null; encoding = null; }
    if (typeof encoding === "function") { cb = encoding; encoding = null; }
    if (!this.hasHeader("content-length") && !this.hasHeader("transfer-encoding")) {
      let body = null;
      if (data != null) {
        body = typeof data === "string" ? new TextEncoder().encode(data) : data;
        this.setHeader("content-length", body.length);
      }
      this._sendHeaders();
      if (body) this.socket.write(body);
    } else {
      this._sendHeaders();
      if (data != null) {
        const encoded = typeof data === "string" ? new TextEncoder().encode(data) : data;
        this.socket.write(encoded);
      }
    }

    this.finished = true;
    this.writableEnded = true;
    this.writableFinished = true;
    this.emit("finish");
    // end() shuts down the write side; once the socket finishes
    // flushing, destroy it to close the read side too.
    var sock = this.socket;
    sock.end();
    sock.once("finish", function () {
      if (!sock.destroyed) sock.destroy();
    });
    if (cb) cb();
    return this;
  }

  addTrailers() {}
  writeContinue() {}
}

class Server extends net.Server {
  constructor(opts, requestListener) {
    if (typeof opts === "function") {
      requestListener = opts;
      opts = {};
    }
    super(opts);
    this.timeout = 0;
    this.keepAliveTimeout = 5000;
    this.maxHeadersCount = 2000;
    this.headersTimeout = 60000;
    this.requestTimeout = 0;

    if (requestListener) this.on("request", requestListener);

    this.on("connection", (socket) => {
      this._handleConnection(socket);
    });
  }

  _handleConnection(socket) {
    let buffer = "";

    const onData = (chunk) => {
      if (typeof chunk !== "string") {
        chunk = new TextDecoder().decode(chunk);
      }
      buffer += chunk;

      const headerEnd = buffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) return;

      const headerSection = buffer.substring(0, headerEnd);
      const bodyStart = buffer.substring(headerEnd + 4);

      const lines = headerSection.split("\r\n");
      const requestLine = lines[0].split(" ");
      const method = requestLine[0];
      const url = requestLine[1];
      const httpVersion = (requestLine[2] || "HTTP/1.1").replace("HTTP/", "");

      const req = new IncomingMessage(socket);
      req.method = method;
      req.url = url;
      req.httpVersion = httpVersion;

      for (let i = 1; i < lines.length; i++) {
        const colonIdx = lines[i].indexOf(":");
        if (colonIdx === -1) continue;
        const key = lines[i].substring(0, colonIdx).trim().toLowerCase();
        const val = lines[i].substring(colonIdx + 1).trim();
        req.rawHeaders.push(lines[i].substring(0, colonIdx).trim(), val);
        if (req.headers[key]) {
          if (Array.isArray(req.headers[key])) {
            req.headers[key].push(val);
          } else {
            req.headers[key] = [req.headers[key], val];
          }
        } else {
          req.headers[key] = val;
        }
      }

      const res = new ServerResponse(req);

      const contentLength = parseInt(req.headers["content-length"] || "0", 10);
      if (contentLength > 0) {
        if (bodyStart.length >= contentLength) {
          req.push(new TextEncoder().encode(bodyStart.substring(0, contentLength)));
          req.push(null);
          req.complete = true;
        } else {
          if (bodyStart.length > 0) req.push(new TextEncoder().encode(bodyStart));
          let received = bodyStart.length;
          const onBodyData = (bodyChunk) => {
            const str = typeof bodyChunk === "string" ? bodyChunk : new TextDecoder().decode(bodyChunk);
            received += str.length;
            req.push(typeof bodyChunk === "string" ? new TextEncoder().encode(bodyChunk) : bodyChunk);
            if (received >= contentLength) {
              socket.removeListener("data", onBodyData);
              req.push(null);
              req.complete = true;
            }
          };
          socket.on("data", onBodyData);
        }
      } else {
        req.push(null);
        req.complete = true;
      }

      socket.removeListener("data", onData);
      buffer = "";

      this.emit("request", req, res);
    };

    socket.on("data", onData);
    socket.on("error", () => {});
  }

  setTimeout(ms, cb) {
    this.timeout = ms;
    if (cb) this.on("timeout", cb);
    return this;
  }
}

function createServer(opts, requestListener) {
  return new Server(opts, requestListener);
}

const STATUS_CODES = {
  100: "Continue", 101: "Switching Protocols",
  200: "OK", 201: "Created", 202: "Accepted", 204: "No Content",
  301: "Moved Permanently", 302: "Found", 304: "Not Modified",
  400: "Bad Request", 401: "Unauthorized", 403: "Forbidden",
  404: "Not Found", 405: "Method Not Allowed", 408: "Request Timeout",
  409: "Conflict", 410: "Gone", 413: "Payload Too Large",
  415: "Unsupported Media Type", 422: "Unprocessable Entity",
  429: "Too Many Requests",
  500: "Internal Server Error", 501: "Not Implemented",
  502: "Bad Gateway", 503: "Service Unavailable", 504: "Gateway Timeout",
};

const METHODS = ["GET", "HEAD", "POST", "PUT", "DELETE", "CONNECT", "OPTIONS", "TRACE", "PATCH"];

class ClientRequest extends Writable {
  constructor(input, options, cb) {
    super();
    if (typeof input === "string") {
      // Simple URL parsing without relying on global URL constructor
      const match = input.match(/^(https?:)?\/\/([^/:]+)(?::(\d+))?(\/[^]*)?$/);
      if (match) {
        input = {
          protocol: match[1] || "http:",
          hostname: match[2],
          port: match[3] ? parseInt(match[3]) : (match[1] === "https:" ? 443 : 80),
          path: match[4] || "/",
        };
      } else {
        input = { path: input };
      }
    }
    if (typeof options === "function") { cb = options; options = {}; }
    options = Object.assign({}, input, options);

    this.method = (options.method || "GET").toUpperCase();
    this.path = options.path || "/";
    this.host = options.hostname || options.host || "localhost";
    this.port = options.port || 80;
    this._headers = {};
    this._headersSent = false;
    this._body = [];
    this.socket = null;
    this.aborted = false;
    this.finished = false;
    this.reusedSocket = false;

    if (options.headers) {
      for (const [k, v] of Object.entries(options.headers)) {
        this._headers[k.toLowerCase()] = v;
      }
    }
    if (!this._headers["host"]) {
      this._headers["host"] = this.port === 80 ? this.host : this.host + ":" + this.port;
    }

    if (cb) this.once("response", cb);

    // Connect on next tick so caller can set headers / write body
    process.nextTick(() => this._connect());
  }

  setHeader(name, value) {
    this._headers[name.toLowerCase()] = value;
    return this;
  }

  getHeader(name) {
    return this._headers[name.toLowerCase()];
  }

  removeHeader(name) {
    delete this._headers[name.toLowerCase()];
  }

  _connect() {
    const socket = new net.Socket();
    this.socket = socket;

    socket.connect(this.port, this.host, () => {
      this.emit("socket", socket);
      this._flush();
    });

    socket.on("error", (err) => this.emit("error", err));
  }

  _flush() {
    // Build and send the HTTP request
    let head = this.method + " " + this.path + " HTTP/1.1\r\n";
    for (const [k, v] of Object.entries(this._headers)) {
      head += k + ": " + v + "\r\n";
    }

    const body = this._body.length > 0
      ? this._body.map(c => typeof c === "string" ? new TextEncoder().encode(c) : c)
      : [];
    const bodyLen = body.reduce((s, c) => s + c.length, 0);
    if (bodyLen > 0 && !this._headers["content-length"] && !this._headers["transfer-encoding"]) {
      head += "content-length: " + bodyLen + "\r\n";
    }
    head += "\r\n";

    this.socket.write(new TextEncoder().encode(head));
    this._headersSent = true;
    for (const chunk of body) {
      this.socket.write(chunk);
    }

    if (this.finished) {
      this._parseResponse();
    }
  }

  _write(chunk, encoding, cb) {
    if (typeof chunk === "string") chunk = new TextEncoder().encode(chunk);
    if (this._headersSent) {
      this.socket.write(chunk);
    } else {
      this._body.push(chunk);
    }
    if (cb) cb();
  }

  write(chunk, encoding, cb) {
    if (typeof encoding === "function") { cb = encoding; encoding = null; }
    if (typeof chunk === "string") chunk = new TextEncoder().encode(chunk);
    this._body.push(chunk);
    if (cb) cb();
    return true;
  }

  end(data, encoding, cb) {
    if (typeof data === "function") { cb = data; data = null; }
    if (typeof encoding === "function") { cb = encoding; encoding = null; }
    if (data != null) {
      if (typeof data === "string") data = new TextEncoder().encode(data);
      this._body.push(data);
    }
    this.finished = true;
    if (this._headersSent || this.socket) {
      this._parseResponse();
    }
    if (cb) this.once("finish", cb);
    return this;
  }

  _parseResponse() {
    let buffer = "";
    let res = null;
    let headersParsed = false;
    let contentLength = -1;
    let bodyReceived = 0;
    let isChunked = false;
    let chunkBuffer = "";

    const onData = (chunk) => {
      const str = typeof chunk === "string" ? chunk : new TextDecoder().decode(chunk);

      if (!headersParsed) {
        buffer += str;
        const headerEnd = buffer.indexOf("\r\n\r\n");
        if (headerEnd === -1) return;

        const headerSection = buffer.substring(0, headerEnd);
        const bodyStart = buffer.substring(headerEnd + 4);

        const lines = headerSection.split("\r\n");
        const statusLine = lines[0].split(" ");
        const httpVersion = (statusLine[0] || "HTTP/1.1").replace("HTTP/", "");
        const statusCode = parseInt(statusLine[1]) || 200;
        const statusMessage = statusLine.slice(2).join(" ") || STATUS_CODES[statusCode] || "";

        res = new IncomingMessage(this.socket);
        res.httpVersion = httpVersion;
        res.statusCode = statusCode;
        res.statusMessage = statusMessage;

        for (let i = 1; i < lines.length; i++) {
          const colonIdx = lines[i].indexOf(":");
          if (colonIdx === -1) continue;
          const key = lines[i].substring(0, colonIdx).trim().toLowerCase();
          const val = lines[i].substring(colonIdx + 1).trim();
          res.rawHeaders.push(lines[i].substring(0, colonIdx).trim(), val);
          res.headers[key] = val;
        }

        headersParsed = true;
        contentLength = parseInt(res.headers["content-length"] || "-1", 10);
        isChunked = (res.headers["transfer-encoding"] || "").toLowerCase().includes("chunked");

        this.emit("response", res);

        if (contentLength === 0) {
          res.push(null);
          res.complete = true;
          this.socket.destroy();
          return;
        }

        if (bodyStart.length > 0) {
          if (isChunked) {
            processChunked(bodyStart);
          } else {
            const bodyBytes = new TextEncoder().encode(bodyStart);
            res.push(bodyBytes);
            bodyReceived += bodyBytes.length;
            if (contentLength >= 0 && bodyReceived >= contentLength) {
              res.push(null);
              res.complete = true;
              this.socket.destroy();
              return;
            }
          }
        }
        return;
      }

      // Body data
      if (isChunked) {
        processChunked(str);
      } else {
        const bodyBytes = typeof chunk === "string" ? new TextEncoder().encode(chunk) : chunk;
        res.push(bodyBytes);
        bodyReceived += bodyBytes.length;
        if (contentLength >= 0 && bodyReceived >= contentLength) {
          res.push(null);
          res.complete = true;
          this.socket.destroy();
        }
      }
    };

    const processChunked = (str) => {
      chunkBuffer += str;
      while (true) {
        const lineEnd = chunkBuffer.indexOf("\r\n");
        if (lineEnd === -1) break;
        const sizeLine = chunkBuffer.substring(0, lineEnd).trim();
        const chunkSize = parseInt(sizeLine, 16);
        if (isNaN(chunkSize)) break;
        if (chunkSize === 0) {
          res.push(null);
          res.complete = true;
          this.socket.destroy();
          return;
        }
        // Need sizeLine + \r\n + chunkSize bytes + \r\n
        const dataStart = lineEnd + 2;
        const dataEnd = dataStart + chunkSize;
        if (chunkBuffer.length < dataEnd + 2) break; // Wait for more data
        const chunkData = chunkBuffer.substring(dataStart, dataEnd);
        res.push(new TextEncoder().encode(chunkData));
        bodyReceived += chunkSize;
        chunkBuffer = chunkBuffer.substring(dataEnd + 2);
      }
    };

    this.socket.on("data", onData);
    this.socket.on("end", () => {
      if (res && !res.complete) {
        res.push(null);
        res.complete = true;
      }
    });
    this.socket.on("error", (err) => {
      this.emit("error", err);
    });
  }

  abort() {
    this.aborted = true;
    if (this.socket) this.socket.destroy();
    this.emit("abort");
  }

  setTimeout(ms, cb) {
    if (cb) this.once("timeout", cb);
    return this;
  }
}

function request(input, options, cb) {
  return new ClientRequest(input, options, cb);
}

function get(input, options, cb) {
  const req = request(input, options, cb);
  req.end();
  return req;
}

const Agent = class Agent {
  constructor(opts) {
    this.maxSockets = (opts && opts.maxSockets) || Infinity;
    this.keepAlive = (opts && opts.keepAlive) || false;
  }
};

const globalAgent = new Agent();

const http = {
  Server, IncomingMessage, ServerResponse, ClientRequest, Agent,
  createServer, request, get,
  STATUS_CODES, METHODS,
  globalAgent,
};

export default http;
export {
  Server, IncomingMessage, ServerResponse, ClientRequest, Agent,
  createServer, request, get,
  STATUS_CODES, METHODS,
};
