/**
 * Tests for cli/patch-wasm-imports.js
 *
 * Run: node --test cli/patch-wasm-imports.test.js
 *
 * Two layers:
 *   1. `patchSource` injects correctly, is idempotent, fails loud on bad input,
 *      and produces syntactically valid JS.
 *   2. The injected `readCustomSection` reads a *real* wasm custom section.
 *      We hand-assemble a minimal module that exports memory and carries our
 *      own custom section, then exercise every branch of the contract. This
 *      stands in for Turbopack's not-yet-emitted sections.
 */
'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');

const { patchSource, readCustomSection, MARKER } = require('./patch-wasm-imports');

// A representative slice of wasm-bindgen `--target web` glue.
const GLUE = [
  'let wasm;',
  'let wasmModule;',
  'function __wbg_init(input) {}',
  'function __wbg_get_imports() {',
  '    const imports = {};',
  '    imports.wbg = {};',
  '    return imports;',
  '}',
  'function initSync(module) {}',
].join('\n');

const GLUE_WITH_ENV_NAMESPACE = [
  'let wasmModule, wasm;',
  'const import1 = {};',
  'function __wbg_get_imports(memory) {',
  '    const import0 = {',
  '        __proto__: null,',
  '        memory: memory || new WebAssembly.Memory({initial:1}),',
  '    };',
  '    return {',
  '        __proto__: null,',
  '        "./index_bg.js": import0,',
  '        "env": import1,',
  '    };',
  '}',
].join('\n');

test('patchSource injects the env.read_custom_section import', () => {
  const { source, patched } = patchSource(GLUE);
  assert.equal(patched, true);
  assert.match(source, /imports\.env = imports\.env \?\? \{\};/);
  assert.match(source, /imports\.env\.read_custom_section =/);
  assert.match(source, new RegExp(`const ${MARKER} =`));
  // wiring lands before the original return, definition before the function.
  assert.ok(source.indexOf(MARKER) < source.indexOf('function __wbg_get_imports('));
  assert.ok(
    source.indexOf('imports.env.read_custom_section') <
      source.indexOf('return imports;'),
  );
});

test('patchSource injects into returned env namespace imports', () => {
  const { source, patched } = patchSource(GLUE_WITH_ENV_NAMESPACE);
  assert.equal(patched, true);
  assert.match(source, /"env": \{/);
  assert.match(source, /\.\.\.import1,/);
  assert.match(
    source,
    /read_custom_section: \(namePtr, nameLength, targetPtr, targetLength\) =>/,
  );
  assert.doesNotMatch(source, /"env": import1,/);
});

test('patched glue is syntactically valid JavaScript', () => {
  const { source } = patchSource(GLUE);
  // Throws on a syntax error; top-level let/function decls are fine in a body.
  assert.doesNotThrow(() => new Function(source));
});

test('patched env namespace glue is syntactically valid JavaScript', () => {
  const { source } = patchSource(GLUE_WITH_ENV_NAMESPACE);
  assert.doesNotThrow(() => new Function(source));
});

test('patchSource is idempotent', () => {
  const once = patchSource(GLUE);
  const twice = patchSource(once.source);
  assert.equal(twice.patched, false);
  assert.equal(twice.source, once.source);
});

test('patchSource throws when anchors are missing', () => {
  assert.throws(() => patchSource('function unrelated() { return 1; }'), /could not find/);
  assert.throws(
    () => patchSource('function __wbg_get_imports() { const imports = {}; }'),
    /return imports;.*env import property/s,
  );
});

// --- real-wasm behavior ---------------------------------------------------

const encoder = new TextEncoder();

// Minimal unsigned LEB128 for the small lengths used here.
function uleb(n) {
  const out = [];
  do {
    let byte = n & 0x7f;
    n >>>= 7;
    if (n !== 0) byte |= 0x80;
    out.push(byte);
  } while (n !== 0);
  return out;
}

function section(id, content) {
  return [id, ...uleb(content.length), ...content];
}

function customSection(name, payload) {
  const nameBytes = encoder.encode(name);
  return section(0x00, [...uleb(nameBytes.length), ...nameBytes, ...payload]);
}

// A valid wasm module: 1 page of exported memory + a custom section.
function buildModule(sectionName, payload) {
  const memName = encoder.encode('memory');
  const bytes = Uint8Array.from([
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // \0asm + version 1
    ...section(0x05, [0x01, 0x00, 0x01]), // memory: 1 mem, limits { min: 1 }
    ...section(0x07, [0x01, memName.length, ...memName, 0x02, 0x00]), // export "memory"
    ...customSection(sectionName, payload),
  ]);
  return new WebAssembly.Module(bytes);
}

function writeString(memory, ptr, str) {
  const bytes = encoder.encode(str);
  new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
  return bytes.length;
}

test('readCustomSection round-trips a real custom section', () => {
  const payload = [10, 20, 30, 40];
  const module = buildModule('ut_test', payload);
  const memory = new WebAssembly.Instance(module).exports.memory;

  const namePtr = 100;
  const nameLen = writeString(memory, namePtr, 'ut_test');

  // Size query (zero-length buffer) returns the required length, copies nothing.
  assert.equal(
    readCustomSection(module, memory, namePtr, nameLen, 0, 0),
    payload.length,
  );

  // Too-small buffer returns the required length, copies nothing.
  const tooSmall = 200;
  assert.equal(
    readCustomSection(module, memory, namePtr, nameLen, tooSmall, 2),
    payload.length,
  );
  assert.deepEqual(
    Array.from(new Uint8Array(memory.buffer, tooSmall, payload.length)),
    [0, 0, 0, 0],
  );

  // Adequate buffer copies the bytes and returns the count.
  const targetPtr = 256;
  assert.equal(
    readCustomSection(module, memory, namePtr, nameLen, targetPtr, 16),
    payload.length,
  );
  assert.deepEqual(
    Array.from(new Uint8Array(memory.buffer, targetPtr, payload.length)),
    payload,
  );
});

test('readCustomSection returns 0 for an unknown section', () => {
  const module = buildModule('ut_test', [1, 2, 3]);
  const memory = new WebAssembly.Instance(module).exports.memory;

  const namePtr = 100;
  const nameLen = writeString(memory, namePtr, 'does_not_exist');
  assert.equal(readCustomSection(module, memory, namePtr, nameLen, 256, 16), 0);
});

test('readCustomSection accepts SharedArrayBuffer-backed wasm memory', () => {
  const payload = [1, 3, 3, 7];
  const module = buildModule('ut_shared', payload);
  const memory = new WebAssembly.Memory({
    initial: 1,
    maximum: 1,
    shared: true,
  });

  const namePtr = 100;
  const nameLen = writeString(memory, namePtr, 'ut_shared');
  const targetPtr = 256;

  assert.equal(
    readCustomSection(module, memory, namePtr, nameLen, targetPtr, 16),
    payload.length,
  );
  assert.deepEqual(
    Array.from(new Uint8Array(memory.buffer, targetPtr, payload.length)),
    payload,
  );
});
