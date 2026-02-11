const Koa = require('koa');
const app = new Koa();

app.use(ctx => {
  ctx.body = 'Hello from Koa on utoo-runtime!';
});

app.listen(3000, () => {
  console.log('Koa listening on http://127.0.0.1:3000');
});
