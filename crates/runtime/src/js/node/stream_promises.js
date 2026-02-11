// node:stream/promises module for utoo-runtime
import Stream, { pipeline, finished } from "ext:utoo_rt_ext/node/stream";

function pipelinePromise(...streams) {
  return new Promise((resolve, reject) => {
    pipeline(...streams, (err) => {
      if (err) reject(err);
      else resolve();
    });
  });
}

function finishedPromise(stream, opts) {
  return new Promise((resolve, reject) => {
    finished(stream, opts, (err) => {
      if (err) reject(err);
      else resolve();
    });
  });
}

const promises = { pipeline: pipelinePromise, finished: finishedPromise };
promises.default = promises;

export default promises;
export { pipelinePromise as pipeline, finishedPromise as finished };
