# with-proxy

Example project demonstrating `devServer.proxy` on the Hono-based dev server.

## Config

See `utoopack.json`: `devServer.proxy` is an array of `ProxyRule`:

- **`/api`** → `https://jsonplaceholder.typicode.com` with `pathRewrite: { "^/api": "" }`
- **`/placeholder`** and **`/json`** → same target with respective path rewrites

## Run

```bash
# From repo root
ut start --workspace with-proxy
# or
npx up --workspace with-proxy
```

Then open the app and use the radio buttons to request `/api/posts/1` or `/placeholder/posts/1`; both are proxied to JSONPlaceholder without CORS.

## proxyFromObject (JS/TS config)

In a JS/TS config file you can use `proxyFromObject` from `@utoo/pack-shared` to build rules from an object:

```ts
import { proxyFromObject } from "@utoo/pack-shared";

export default {
  devServer: {
    proxy: [
      ...proxyFromObject({
        "/api": "http://localhost:3000",
        "/auth": { target: "http://localhost:5000", changeOrigin: true },
      }),
    ],
  },
};
```
