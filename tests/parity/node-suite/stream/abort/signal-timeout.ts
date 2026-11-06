import { Readable, pipeline } from "node:stream";
// AbortSignal.timeout(ms) aborts after ms milliseconds; pipeline gets
// the abort as an err.
const signal = (AbortSignal as any).timeout(20);
const src = new Readable({ read() {} }); // never pushes
pipeline(src, async function* (s: AsyncIterable<any>) {
  for await (const c of s) yield c;
}, { signal }, (err: any) => {
  console.log("err present:", !!err);
  console.log("err name:", err && err.name);
});
