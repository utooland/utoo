import { Application } from 'egg';

const app = new Application({
  baseDir: __dirname,
  type: 'application',
  mode: 'single',
});

const port: number = parseInt(process.env.PORT || '3000', 10);
app.ready(() => {
  app.listen(port, '0.0.0.0', () => {
    console.log(`Egg.js listening on http://0.0.0.0:${port}`);
  });
});
