// stream/promises pipeline(source, transform, finalFn) reached through
// req("stream/promises"): the final function must run and the promise must
// resolve with its return value, whichever way the export is read (esbuild output calls it as
// `(0, import_promises.pipeline)(body, limit, handler)`).
import helper from "./gap_stream_pipeline_final_fn_require_helper.cjs";
"use strict";
const { stream, bare, prefixed } = helper;
const { pipeline: destructured } = bare;

const passThrough = () =>
  new stream.Transform({ transform(chunk, enc, cb) { cb(null, chunk); } });
const body = () => stream.Readable.from([Buffer.alloc(1000), Buffer.alloc(500)]);
const total = async (s) => {
  let n = 0;
  for await (const chunk of s) n += chunk.length;
  return { headers: { etag: n } };
};
const timeout = (p, ms) =>
  Promise.race([p, new Promise((resolve) => setTimeout(() => resolve("TIMEOUT"), ms))]);

const tests = {
  member: () => bare.pipeline(body(), passThrough(), total),
  prefixedMember: () => prefixed.pipeline(body(), passThrough(), total),
  comma: () => (0, bare.pipeline)(body(), passThrough(), total),
  destructured: () => destructured(body(), passThrough(), total),
  streamPromises: () => stream.promises.pipeline(body(), passThrough(), total),
  sameFunction: () => Promise.resolve(bare.pipeline === stream.promises.pipeline),
};
(async () => {
  for (const [name, run] of Object.entries(tests)) {
    console.log(name, JSON.stringify(await timeout(run(), 3000)));
  }
  process.exit(0);
})();
