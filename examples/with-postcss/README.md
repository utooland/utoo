# with-postcss

This example demonstrates utoopack's inline `styles.postcss` configuration in
`utoopack.config.mjs`.

It passes `postcss-plugin-px2rem` directly through `utoopack.config.mjs` instead of
using a separate `postcss.config.js` file.

To verify it locally:

```bash
npm install
npm run dev
```

Then inspect the emitted CSS and confirm `px` values like `40px`, `32px`, and
`24px` are transformed into `rem` values.
