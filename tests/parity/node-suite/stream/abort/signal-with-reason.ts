import { Readable, pipeline } from "node:stream";
// AbortController.abort(reason) propagates the custom reason to the
// stream pipeline callback error.
const ctrl = new AbortController();
const customReason = new Error("user-cancel");
const src = new Readable({ read() {} });
pipeline(src, async function* (s: AsyncIterable<any>) {
  for await (const c of s) yield c;
}, { signal: ctrl.signal }, (err: any) => {
  console.log("err present:", !!err);
  console.log("err message:", err && err.message);
});
ctrl.abort(customReason);
