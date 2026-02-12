// Test: Dig into Next.js dev server route resolution
process.on('uncaughtException', (err) => {
  console.error('[UNCAUGHT]', err.name, err.message);
  console.error(err.stack?.split('\n').slice(0,6).join('\n'));
});

const http = require('http');
const next = require('next');

const app = next({ dev: true, dir: __dirname });
const handle = app.getRequestHandler();

app.prepare().then(async () => {
  // NextServer wraps the real server - access it
  console.log('=== Outer NextServer ===');
  console.log('app keys:', Object.keys(app).slice(0, 20));

  // Try to get the internal server
  let inner = null;
  try {
    // NextServer stores the server promise internally
    if (app.serverPromise) {
      inner = await app.serverPromise;
    } else if (app.server) {
      inner = await app.server;
    }
  } catch(e) {}

  // Try accessing via getServer() if available
  if (!inner && app.getServer) {
    try { inner = await app.getServer(); } catch(e) {}
  }

  if (!inner) {
    // Look for private fields
    for (const k of Object.keys(app)) {
      const v = app[k];
      if (v && typeof v === 'object' && v.constructor && v.constructor.name.includes('Server')) {
        console.log('Found server-like on app.' + k + ':', v.constructor.name);
        inner = v;
      }
      if (v && typeof v.then === 'function') {
        console.log('Found promise on app.' + k);
      }
    }
  }

  console.log('\n=== Inner server ===');
  console.log('inner:', inner?.constructor?.name || 'null');
  if (inner) {
    console.log('inner keys:', Object.keys(inner).sort().join(', '));

    // Check matchers
    if (inner.matchers) {
      console.log('\nmatchers:', inner.matchers.constructor?.name);
      try {
        await inner.matchers.reload();
        console.log('matchers reloaded');
      } catch(e) { console.log('matchers reload error:', e.message); }

      try {
        let count = 0;
        for await (const m of inner.matchers.matchAll('/', { skipFilesystemCheck: true })) {
          console.log('match:', JSON.stringify(m?.definition || { page: m?.pathname }));
          count++;
        }
        console.log('total matches for /:', count);
      } catch(e) { console.log('matchAll error:', e.message); }
    }

    // Check ensurePage
    if (inner.ensurePage) {
      console.log('\n--- ensurePage for / ---');
      try {
        const r = await inner.ensurePage({ page: '/', clientOnly: false });
        console.log('ensurePage result:', r);
      } catch(e) {
        console.error('ensurePage error:', e.message);
        console.error(e.stack?.split('\n').slice(0,6).join('\n'));
      }
    }

    // Check hotReloader
    if (inner.hotReloader) {
      console.log('\nhotReloader type:', inner.hotReloader.constructor?.name);
      console.log('hotReloader keys:', Object.keys(inner.hotReloader).slice(0,20));
    }

    // Check appPathRoutes
    console.log('\nappPathRoutes:', inner.appPathRoutes);
    if (inner.getAppPathRoutes) {
      console.log('getAppPathRoutes():', inner.getAppPathRoutes());
    }

    // Check renderOpts
    if (inner.renderOpts) {
      console.log('\nrenderOpts.dev:', inner.renderOpts.dev);
    }
  }

  // Start minimal server
  const srv = http.createServer((req, res) => {
    console.log('\n[REQ] ' + req.method + ' ' + req.url);
    handle(req, res).catch((err) => {
      console.error('[REQ-ERR]', err.message);
    });
  });
  srv.listen(3456, '127.0.0.1', () => {
    console.log('\nListening on :3456');
    http.get('http://127.0.0.1:3456/hello', (res) => {
      console.log('[RESP] status:', res.statusCode);
      let body = '';
      res.setEncoding('utf-8');
      res.on('data', (d) => body += d);
      res.on('end', () => {
        console.log('[RESP] body:', body.slice(0, 500));
        srv.close();
        process.exit(0);
      });
    }).on('error', (e) => {
      console.error('[RESP] error:', e.message);
      srv.close();
      process.exit(1);
    });
  });
}).catch(e => {
  console.error('prepare error:', e.message);
  console.error(e.stack);
});
