/**
 * patch-wasm-imports.js
 *
 * Post-`wasm-bindgen` codemod for `@utoo/web`.
 *
 * The `link-section` crate (pulled in transitively by Turbopack on wasm32)
 * declares a host import `(import "env" "read_custom_section" (func ...))`.
 * wasm-bindgen leaves that import in the module but does NOT provide it in the
 * imports object it builds, so without help the module fails to instantiate
 * with a `LinkError`.
 *
 * This codemod injects the missing `env.read_custom_section` host function into
 * the generated glue's `__wbg_get_imports()` — the single place where both the
 * main `__wbg_init` path and the worker `initSync` path build their imports, so
 * one patch covers every instantiation site.
 *
 * The host function reads out-of-band wasm custom sections (which the wasm
 * runtime itself cannot see) via `WebAssembly.Module.customSections` and copies
 * the bytes into linear memory. See:
 *   https://docs.rs/link-section/latest/link_section/#wasm
 *
 * The generated glue (`src/utoo/index.js`) is regenerated on every build and is
 * git-ignored, so this runs as a step right after each `bindgen-*` script.
 *
 * Usage: node cli/patch-wasm-imports.js [path/to/index.js]
 */
'use strict';

const fs = require('fs');
const path = require('path');

// Name of the injected reader; also used as the idempotency marker.
const MARKER = '__utooReadCustomSection';

/**
 * Host implementation of the `read_custom_section` import.
 *
 * Follows the contract expected by the `link-section` crate:
 *   - return 0 when no section with that name exists;
 *   - return the required byte length when the caller's buffer is too small
 *     (the caller then retries with a large-enough buffer);
 *   - otherwise copy the section bytes into `[targetPtr, targetPtr + need)` and
 *     return the number of bytes written.
 *
 * Kept dependency-free (only the `TextDecoder` / `WebAssembly` / `Uint8Array`
 * globals, available in browsers and workers) so it can be both stringified
 * into the glue below and unit-tested directly from this module.
 */
function readCustomSection(
  wasmModule,
  memory,
  namePtr,
  nameLength,
  targetPtr,
  targetLength,
) {
  const nameBytes = new Uint8Array(memory.buffer, namePtr, nameLength);
  const sectionName = new TextDecoder('utf-8').decode(
    // TextDecoder rejects SharedArrayBuffer-backed views in browsers.
    new Uint8Array(nameBytes),
  );
  const sections = WebAssembly.Module.customSections(wasmModule, sectionName);
  if (sections.length === 0) {
    return 0;
  }
  const section = sections[0];
  const need = section.byteLength;
  if (targetLength < need) {
    return need;
  }
  new Uint8Array(memory.buffer, targetPtr, need).set(new Uint8Array(section));
  return need;
}

// `function __wbg_get_imports(` — match regardless of args/spacing tweaks.
const FN_ANCHOR = 'function __wbg_get_imports(';
// Last statement of older `__wbg_get_imports` output.
const RETURN_ANCHOR = 'return imports;';
// Current wasm-bindgen emits a returned import object with `"env": import1`.
const ENV_PROPERTY_RE = /(\n\s*)(["']env["']\s*:\s*)([$A-Z_a-z][$\w]*)(\s*,)/;

// Reader definition, injected once just before `__wbg_get_imports`. The inner
// function name is scoped to the expression, so it cannot collide with glue
// symbols. `wasm` (the glue's `let wasm` holding `instance.exports`) and
// `wasmModule` (the compiled module, set by `__wbg_finalize_init`) are both
// populated before wasm-bindgen manually calls the wasm start function — and
// therefore before the `link-section` ctors run — so they are safe to read
// lazily from the host import.
const READER_DEF = `// --- utoo: link-section host import (read_custom_section) ---
// Injected by cli/patch-wasm-imports.js. Supplies the \`env.read_custom_section\`
// import expected by the \`link-section\` crate on wasm32 (reads out-of-band wasm
// custom sections). See https://docs.rs/link-section/latest/link_section/#wasm
const ${MARKER} = ${readCustomSection.toString()};`;

// Wiring injected inside `__wbg_get_imports`, just before `return imports;`.
// Only sets a single property on `imports.env`, never replacing the object, so
// it coexists with any `env`/memory imports wasm-bindgen may add elsewhere.
const IMPORT_WIRING =
  `imports.env = imports.env ?? {};\n` +
  `    imports.env.read_custom_section = (namePtr, nameLength, targetPtr, targetLength) =>\n` +
  `        ${MARKER}(wasmModule, wasm.memory, namePtr, nameLength, targetPtr, targetLength);\n`;

function buildEnvObject(indent, envIdentifier) {
  return (
    `{\n` +
    `${indent}    __proto__: null,\n` +
    `${indent}    ...${envIdentifier},\n` +
    `${indent}    read_custom_section: (namePtr, nameLength, targetPtr, targetLength) =>\n` +
    `${indent}        ${MARKER}(wasmModule, wasm.memory, namePtr, nameLength, targetPtr, targetLength),\n` +
    `${indent}}`
  );
}

/**
 * Inject the `env.read_custom_section` host import into wasm-bindgen
 * `--target web` glue source. Idempotent. Throws if the expected anchors are
 * missing (so a wasm-bindgen output change fails the build loudly instead of
 * silently producing a module that cannot instantiate).
 *
 * @param {string} source generated glue (`index.js`) contents
 * @returns {{ source: string, patched: boolean }}
 */
function patchSource(source) {
  if (source.includes(MARKER)) {
    return { source, patched: false };
  }

  const fnStart = source.indexOf(FN_ANCHOR);
  if (fnStart === -1) {
    throw new Error(
      `patch-wasm-imports: could not find '${FN_ANCHOR}' in the generated glue; ` +
        'wasm-bindgen output shape may have changed.',
    );
  }

  const rest = source.slice(fnStart);

  let patchedRest;
  if (rest.indexOf(RETURN_ANCHOR) !== -1) {
    patchedRest = rest.replace(
      RETURN_ANCHOR,
      `${IMPORT_WIRING}    ${RETURN_ANCHOR}`,
    );
  } else if (ENV_PROPERTY_RE.test(rest)) {
    patchedRest = rest.replace(
      ENV_PROPERTY_RE,
      (match, indent, propPrefix, envIdentifier, suffix) =>
        `${indent}${propPrefix}${buildEnvObject(indent, envIdentifier)}${suffix}`,
    );
  } else {
    throw new Error(
      `patch-wasm-imports: could not find '${RETURN_ANCHOR}' or an env import property ` +
        'inside __wbg_get_imports(); wasm-bindgen output shape may have changed.',
    );
  }

  const out = `${source.slice(0, fnStart)}${READER_DEF}\n\n${patchedRest}`;
  return { source: out, patched: true };
}

function main() {
  const target =
    process.argv[2] ?? path.resolve(process.cwd(), 'src/utoo/index.js');

  let source;
  try {
    source = fs.readFileSync(target, 'utf8');
  } catch (err) {
    throw new Error(
      `patch-wasm-imports: cannot read generated glue at ${target}: ${err.message}`,
    );
  }

  const { source: out, patched } = patchSource(source);
  if (patched) {
    fs.writeFileSync(target, out);
    console.log(
      `patch-wasm-imports: injected env.read_custom_section into ${target}`,
    );
  } else {
    console.log(`patch-wasm-imports: ${target} already patched, skipping`);
  }
}

if (require.main === module) {
  main();
}

module.exports = { patchSource, readCustomSection, MARKER };
