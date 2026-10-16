// `Readable.toWeb` reached through a CommonJS `require("node:stream")`
// namespace (Next.js's stream-ops.node.js) returns a real ReadableStream:
// `tee`, `getReader` and async iteration all work. Perry's static-method
// value for toWeb returned a `{ getReader, cancel }` stub, so Next.js's
// `nodeReadableToWebReadableStream(prelude).tee()` / `for await` threw and
// dynamic pages sent an empty reply.
import * as ops from "./stream_to_web_cjs/ops.cjs";

const dec = new TextDecoder();
// The program uses web streams itself (as Next.js does).
const probe = new ReadableStream({ start(c) { c.close(); } });

async function main() {
  const web: any = ops.toWeb(ops.from(["a", "b"]));
  console.log(typeof web.tee, typeof web.getReader, web instanceof ReadableStream, typeof probe.tee);
  const [x, y] = web.tee();
  const r = x.getReader();
  let s = "";
  for (;;) { const { done, value } = await r.read(); if (done) break; s += dec.decode(value); }
  let t = "";
  for await (const c of y) t += dec.decode(c);
  console.log("tee", s, t);
  let u = "";
  for await (const c of ops.toWeb(ops.from(["c", "d"])) as any) u += dec.decode(c);
  console.log("iter", u);
}
main();
