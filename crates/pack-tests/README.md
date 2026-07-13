# pack-tests

An extracted create to perform snapshot tests on utoopack.

## Testing

Before test, please install `cargo-nextest`:

```bash
cargo install cargo-nextest
```

It's possible to only run the snapshot tests using [nextest][]'s filter
expressions:

```bash
cargo nextest run -E 'test(snapshot)'
```

The filter supports any substring, and only test names which contain
that substring will run.

## Updating Snapshot

If you've made a change that requires many snapshot updates, you can
automatically update all outputs using the `UPDATE` command line env:

```bash
UPDATE=1 cargo nextest run -E 'test(snapshot)'
```

## Runtime Assertions

Add an `assert.js` file to a snapshot case when the generated output must also
be executed. The test runner invokes it with Node.js from the case directory
after the output matches its snapshot.

[nextest]: https://nexte.st/
