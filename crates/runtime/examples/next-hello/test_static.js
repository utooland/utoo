const send = require('send');
const http = require('http');
const path = require('path');

const root = path.join(__dirname, '.next/dev/static');
console.log('serving from:', root);

http.createServer((req, res) => {
  console.log('[REQ]', req.url);
  const stream = send(req, req.url, { root });
  stream.on('error', (err) => {
    console.error('[SEND-ERR] code:', err.code, 'status:', err.statusCode, 'msg:', err.message);
    console.error('[SEND-ERR] stack:', (err.stack || '').split('\n').slice(0, 5).join('\n'));
    res.statusCode = err.statusCode || 500;
    res.end(err.message);
  });
  stream.on('directory', () => {
    console.log('[DIR]');
    res.statusCode = 403;
    res.end('Forbidden');
  });
  stream.on('file', (p) => {
    console.log('[FILE]', p);
  });
  stream.on('stream', () => {
    console.log('[STREAM] piping');
  });
  stream.pipe(res);
}).listen(3002, '0.0.0.0', () => {
  console.log('listening on 3002');
});
