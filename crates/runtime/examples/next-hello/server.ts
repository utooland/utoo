import { createRequire } from 'module';
import { fileURLToPath } from 'url';
import { dirname } from 'path';

const require = createRequire(import.meta.url);
const __dirname: string = dirname(fileURLToPath(import.meta.url));

const { createServer } = require('http');
const next = require('next');

const port: number = parseInt(process.env.PORT || '3000', 10);
const hostname: string = process.env.HOST || '0.0.0.0';
const dev: boolean = process.env.NODE_ENV !== 'production';

const app = next({ dev, dir: __dirname });
const handle = app.getRequestHandler();

app.prepare().then(() => {
  createServer((req: any, res: any) => {
    console.log(`[REQ] ${req.method} ${req.url}`);
    handle(req, res).catch((err: Error) => {
      console.error('[SERVER] Request handler error:', err);
      if (!res.headersSent) {
        res.statusCode = 500;
        res.end('Internal Server Error');
      }
    });
  }).listen(port, hostname, () => {
    console.log(`Next.js listening on http://${hostname}:${port}`);
  });
}).catch((err: Error) => {
  console.error('[SERVER] Failed to prepare Next.js:', err);
});
