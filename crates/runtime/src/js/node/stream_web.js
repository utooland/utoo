// node:stream/web module for utoo-runtime
// Exposes Web Streams API - use globalThis if available, otherwise stub

const ReadableStream = globalThis.ReadableStream || class ReadableStream {
  constructor() { throw new Error("ReadableStream not available"); }
};

const WritableStream = globalThis.WritableStream || class WritableStream {
  constructor() { throw new Error("WritableStream not available"); }
};

const TransformStream = globalThis.TransformStream || class TransformStream {
  constructor() { throw new Error("TransformStream not available"); }
};

const ReadableStreamDefaultReader = globalThis.ReadableStreamDefaultReader || class ReadableStreamDefaultReader {};
const ReadableStreamBYOBReader = globalThis.ReadableStreamBYOBReader || class ReadableStreamBYOBReader {};
const ReadableStreamDefaultController = globalThis.ReadableStreamDefaultController || class ReadableStreamDefaultController {};
const ReadableByteStreamController = globalThis.ReadableByteStreamController || class ReadableByteStreamController {};
const WritableStreamDefaultWriter = globalThis.WritableStreamDefaultWriter || class WritableStreamDefaultWriter {};
const WritableStreamDefaultController = globalThis.WritableStreamDefaultController || class WritableStreamDefaultController {};
const TransformStreamDefaultController = globalThis.TransformStreamDefaultController || class TransformStreamDefaultController {};
const ByteLengthQueuingStrategy = globalThis.ByteLengthQueuingStrategy || class ByteLengthQueuingStrategy {
  constructor({ highWaterMark }) { this.highWaterMark = highWaterMark; }
  size(chunk) { return chunk.byteLength; }
};
const CountQueuingStrategy = globalThis.CountQueuingStrategy || class CountQueuingStrategy {
  constructor({ highWaterMark }) { this.highWaterMark = highWaterMark; }
  size() { return 1; }
};

const web = {
  ReadableStream,
  WritableStream,
  TransformStream,
  ReadableStreamDefaultReader,
  ReadableStreamBYOBReader,
  ReadableStreamDefaultController,
  ReadableByteStreamController,
  WritableStreamDefaultWriter,
  WritableStreamDefaultController,
  TransformStreamDefaultController,
  ByteLengthQueuingStrategy,
  CountQueuingStrategy,
};
web.default = web;

export default web;
export {
  ReadableStream,
  WritableStream,
  TransformStream,
  ReadableStreamDefaultReader,
  ReadableStreamBYOBReader,
  ReadableStreamDefaultController,
  ReadableByteStreamController,
  WritableStreamDefaultWriter,
  WritableStreamDefaultController,
  TransformStreamDefaultController,
  ByteLengthQueuingStrategy,
  CountQueuingStrategy,
};
