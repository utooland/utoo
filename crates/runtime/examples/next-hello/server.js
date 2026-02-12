// Capture unhandled errors
process.on('uncaughtException', (err) => {
  console.error('[UNCAUGHT]', err.name, err.message, '\n', err.stack);
});

// Intercept console.error to catch the original error before digesting
const _origError = console.error;
console.error = function(...args) {
  // If first arg is an Error, log its details
  if (args[0] instanceof Error) {
    _origError.call(console, '[ERR-INTERCEPT]', args[0].name + ':', args[0].message);
    _origError.call(console, '[ERR-INTERCEPT] stack:', args[0].stack?.split('\n').slice(0,8).join('\n'));
  }
  // Look for error objects with digest property
  for (const arg of args) {
    if (arg && typeof arg === 'object' && arg.digest && arg.message) {
      _origError.call(console, '[DIGEST-ERR]', arg.name + ':', arg.message, 'digest:', arg.digest);
      _origError.call(console, '[DIGEST-ERR] stack:', (arg.stack || '').split('\n').slice(0,8).join('\n'));
    }
  }
  return _origError.apply(console, args);
};

// Intercept http response to capture flight data
const _origWrite = require('http').ServerResponse.prototype.write;
const _origEnd = require('http').ServerResponse.prototype.end;
let _flightLog = [];
require('http').ServerResponse.prototype.write = function(chunk, encoding, cb) {
  if (typeof chunk === 'string' && chunk.length < 2000) {
    // Log text flight data
    const lines = chunk.split('\n').filter(l => l.length > 0);
    for (const line of lines.slice(0, 5)) {
      _origError.call(console, '[FLIGHT-DATA]', line.slice(0, 200));
    }
    if (lines.length > 5) {
      _origError.call(console, '[FLIGHT-DATA] ...and', lines.length - 5, 'more lines');
    }
  } else if (chunk instanceof Uint8Array || Buffer.isBuffer(chunk)) {
    const text = Buffer.from(chunk).toString('utf-8');
    if (text.length < 2000) {
      const lines = text.split('\n').filter(l => l.length > 0);
      for (const line of lines.slice(0, 5)) {
        _origError.call(console, '[FLIGHT-BIN]', line.slice(0, 200));
      }
    } else {
      _origError.call(console, '[FLIGHT-BIN] chunk size:', chunk.length);
    }
  }
  return _origWrite.call(this, chunk, encoding, cb);
};

const { createServer } = require('http');
const next = require('next');

const port = parseInt(process.env.PORT || '3000', 10);
const hostname = process.env.HOST || 'localhost';
const dev = process.env.NODE_ENV !== 'production';

const app = next({ dev, dir: __dirname });
const handle = app.getRequestHandler();

app.prepare().then(() => {
  createServer((req, res) => {
    console.log(`[REQ] ${req.method} ${req.url}`);
    handle(req, res).catch((err) => {
      console.error('[SERVER] Request handler error:', err);
      if (!res.headersSent) {
        res.statusCode = 500;
        res.end('Internal Server Error');
      }
    });
  }).listen(port, hostname, () => {
    console.log(`Next.js listening on http://${hostname}:${port}`);
  });
}).catch((err) => {
  console.error('[SERVER] Failed to prepare Next.js:', err);
});
