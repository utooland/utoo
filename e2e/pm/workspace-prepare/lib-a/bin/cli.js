#!/usr/bin/env node
// Fixture bin for the workspace-prepare e2e case. Its presence gives `lib-a`
// a `bin`, which is what trips the rebuild collector's gate and exposed the
// 3× workspace install-hook duplication in #3097.
console.log("lib-a-cli");
