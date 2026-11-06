# Utoopack Bundle Analyzer UI

This private workspace packages the MIT-licensed Turbopack Bundle Analyzer from
`next.js/apps/bundle-analyzer`, first adopted at Next.js commit
`f77675fd18a3c9b3138c44eca0d053b9c44533d6`. The build uses the same checked-out
submodule as the Rust bundler. It compiles the original treemap, sidebar, import
chains, file search, and binary data readers into the static files shipped by
`@utoo/pack`. The built report includes the upstream license.

The standalone page supplies the named client and server entries written by Pack.
The environment selector filters the entry list and source modules. Its
entry picker is adapted from upstream's route picker, and its graph starts at the
exact module roots written by the native analyzer. The graph's initial and async
classifications describe import paths; they do not measure browser requests.

To rebuild just the static assets, initialize the `next.js` submodule, install
workspace dependencies, and run:

```sh
npm run build:analyzer --workspace=@utoo/pack
```
