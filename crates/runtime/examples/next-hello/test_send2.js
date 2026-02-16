const send = require('send');
const http = require('http');
const EventEmitter = require('events');

const root = __dirname + '/.next/dev/static';

console.log('creating send stream...');
const req = new EventEmitter();
req.method = 'GET';
req.url = '/development/_buildManifest.js';
req.headers = { host: 'localhost:3000' };

const s = send(req, req.url, { root });
console.log('send type:', s.constructor?.name);

s.on('error', (err) => console.log('error:', err.statusCode, err.message));
s.on('directory', () => console.log('directory'));
s.on('file', (p, stat) => console.log('file:', p, 'size:', stat?.size));
s.on('headers', (res, p) => console.log('headers:', p));
s.on('stream', (stream) => console.log('stream event'));
s.on('end', () => console.log('end event'));

// Create mock writable dest
const chunks = [];
const mockRes = new EventEmitter();
mockRes.setHeader = (k, v) => {};
mockRes.getHeader = (k) => undefined;
mockRes.removeHeader = (k) => {};
mockRes.statusCode = 200;
mockRes.headersSent = false;
mockRes.write = (chunk) => { chunks.push(chunk); return true; };
mockRes.end = (data) => { if (data) chunks.push(data); console.log('res.end, total bytes:', chunks.reduce((a,c) => a + (c.length || 0), 0)); };

console.log('piping...');
s.pipe(mockRes);
console.log('piped');

setTimeout(() => {
  console.log('timeout - chunks:', chunks.length);
  process.exit(0);
}, 3000);
