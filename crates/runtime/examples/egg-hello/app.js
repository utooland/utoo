const { Application } = require('egg');

const app = new Application({
  baseDir: __dirname,
  type: 'application',
  mode: 'single',
});

app.ready(() => {
  app.listen(3000, () => {
    console.log('Egg.js listening on http://127.0.0.1:3000');
  });
});
