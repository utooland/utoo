import Koa from 'koa';

const app = new Koa();

app.use((ctx: any) => {
  ctx.body = 'Hello from Koa on utoo-runtime!';
});

const port: number = parseInt(process.env.PORT || '3000', 10);
app.listen(port, '0.0.0.0', () => {
  console.log(`Koa listening on http://0.0.0.0:${port}`);
});
