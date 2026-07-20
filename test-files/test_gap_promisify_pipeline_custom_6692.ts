// #6692: `promisify(stream.pipeline)` must honor Node's
// `stream.pipeline[util.promisify.custom]` hook (the promise-based
// `stream/promises` impl) instead of falling back to the generic
// callback-appending wrapper. Same for `stream.finished`.
import { promisify } from "util";
import { pipeline, finished, Readable, Writable } from "stream";

const CUSTOM = Symbol.for("nodejs.util.promisify.custom");
console.log("pipeline custom:", typeof (pipeline as any)[CUSTOM]);
console.log("finished custom:", typeof (finished as any)[CUSTOM]);

const p = promisify(pipeline);
const f = promisify(finished);

async function main() {
  // promisified pipeline forwards all stream args and resolves with undefined.
  const chunks: string[] = [];
  const result = await p(
    Readable.from(["a", "b", "c"]),
    new Writable({
      write(c: any, _e: any, cb: any) {
        chunks.push(c.toString());
        cb();
      },
    }),
  );
  console.log("pipeline output:", chunks.join(""));
  console.log("pipeline resolved:", String(result));

  // promisified pipeline with a transform stage in the middle.
  const { Transform } = await import("stream");
  const upper: string[] = [];
  await p(
    Readable.from(["d", "e"]),
    new Transform({
      transform(c: any, _e: any, cb: any) {
        cb(null, c.toString().toUpperCase());
      },
    }),
    new Writable({
      write(c: any, _e: any, cb: any) {
        upper.push(c.toString());
        cb();
      },
    }),
  );
  console.log("transform output:", upper.join(""));

  // promisified finished resolves once the stream is fully consumed.
  const r = Readable.from(["x"]);
  r.resume();
  await f(r);
  console.log("finished resolved");
}

main().then(
  () => console.log("done"),
  (err) => console.log("error:", err && err.code, err && err.message),
);
