# Shared build-time runtime regression

This integration fixture builds an asynchronous PostCSS evaluator graph followed by an independent
synchronous loader graph. Both emit the same build-time Turbopack runtime file.

The PostCSS config uses top-level await so its graph needs `__turbopack_context__.a()`. The second
build deterministically exercises the overwrite from utooland/utoo#3316: every independent graph
must retain `contextPrototype.a = asyncModule` in the shared runtime.
