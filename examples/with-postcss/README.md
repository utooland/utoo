# with-postcss

This example demonstrates utoopack's merged PostCSS configuration.

`postcss.config.js` supplies `postcss-nested`, while `styles.postcss.plugins`
supplies `postcss-plugin-px2rem`. Utoopack runs the file plugins first and
appends the inline plugins in the same PostCSS pass.

To verify it locally:

```bash
npm install
npm run dev
```

Then inspect the emitted CSS and confirm nested selectors are flattened and `px`
values like `40px`, `32px`, and `24px` are transformed into `rem` values.
