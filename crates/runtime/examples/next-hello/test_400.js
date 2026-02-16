// Patch to intercept where 400 is set
const origDefProp = Object.defineProperty;
const http = require('http');
const _origStatusCode = Object.getOwnPropertyDescriptor(http.ServerResponse.prototype, 'statusCode');

const { createServer } = http;
const next = require('next');

const app = next({ dev: true, dir: __dirname });
const handle = app.getRequestHandler();

app.prepare().then(() => {
  createServer((req, res) => {
    // Intercept statusCode setting
    let _sc = 200;
    Object.defineProperty(res, 'statusCode', {
      get() { return _sc; },
      set(v) {
        if (v === 400) {
          console.log('[400 SET] url:', req.url);
          console.log('[400 SET] stack:', new Error().stack.split('\n').slice(1, 8).join('\n'));
        }
        _sc = v;
      },
      configurable: true
    });
    handle(req, res).catch((err) => {
      console.error('[ERR]', err.message);
    });
  }).listen(3001, '0.0.0.0', () => {
    console.log('listening on 3001');
  });
}).catch(e => console.error('prepare:', e));
