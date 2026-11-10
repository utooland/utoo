# Bundle analysis data

The binary report serializer in `../analyze.rs` and the supporting `groups.rs`,
`membership.rs`, and `output_file.rs` are adapted from Next.js
[`crates/next-api/src/analyze.rs`](https://github.com/vercel/next.js/blob/8d366115d9/crates/next-api/src/analyze.rs)
and [its submodules](https://github.com/vercel/next.js/tree/8d366115d9/crates/next-api/src/analyze)
at the upstream base of the pinned Turbopack submodule. They are covered by the
upstream MIT license reproduced below. The Pack-specific entry discovery and
report emission live in `capture.rs`.

Keep this writer and the analyzer UI parser on the same schema version. Pack
uses configured browser entry names for labels and controlled route IDs for
report paths. Bootstrap groups represent the evaluated entry group's direct
assets; async groups are derived from typed loader references. The report
describes build output, not observed browser requests. As in Next.js, analysis
generates output source maps without chaining input source maps so that source
attribution and module graph paths identify the same resolved files.

## Upstream license

The MIT License (MIT)

Copyright (c) 2025 Vercel, Inc.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
