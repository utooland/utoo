// node:stream/consumers module for utoo-runtime
import { Readable } from "ext:utoo_rt_ext/node/stream";

async function arrayBuffer(stream) {
  const chunks = [];
  for await (const chunk of stream) chunks.push(chunk);
  const totalLength = chunks.reduce((acc, c) => acc + c.length, 0);
  const result = new Uint8Array(totalLength);
  let offset = 0;
  for (const chunk of chunks) {
    const bytes = typeof chunk === "string" ? new TextEncoder().encode(chunk) : chunk;
    result.set(bytes, offset);
    offset += bytes.length;
  }
  return result.buffer;
}

async function blob(stream) {
  const ab = await arrayBuffer(stream);
  return new Blob([ab]);
}

async function buffer(stream) {
  const ab = await arrayBuffer(stream);
  return Buffer.from(ab);
}

async function json(stream) {
  const text_ = await text(stream);
  return JSON.parse(text_);
}

async function text(stream) {
  const chunks = [];
  for await (const chunk of stream) {
    chunks.push(typeof chunk === "string" ? chunk : new TextDecoder().decode(chunk));
  }
  return chunks.join("");
}

const consumers = { arrayBuffer, blob, buffer, json, text };
consumers.default = consumers;

export default consumers;
export { arrayBuffer, blob, buffer, json, text };
