const { createServer } = require('http');
createServer((req, res) => {
  console.log('url:', JSON.stringify(req.url));
  console.log('method:', req.method);
  console.log('headers:', JSON.stringify(req.headers));
  res.writeHead(200, { 'Content-Type': 'text/plain' });
  res.end('ok');
}).listen(3001, '0.0.0.0', () => {
  console.log('listening on 3001');
});
