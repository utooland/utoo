### A simple example to use @utoo/web. Steps:

```sh
cd ../../packages/utoo-web
npm install
npm run install-toolchain
npm run dev # or npm run build
cd ../../examples/utooweb-demo
npm run start
```

The embedded project also demonstrates a minimal evjs full-stack build: the
browser bundle is written to `dist`, and the Fetch handler is written to
`dist-server/index.<contenthash>.js`. The order button calls the generated
server-function proxy when the Fetch handler is mounted on the same origin.
