import { PassThrough, pipeline } from "node:stream";
// pipeline(...) wires streams end-to-end and fires its callback with `null`
// on success (or an Error on failure).
const src = new PassThrough();
const sink = new PassThrough();
const out: string[] = [];
sink.on("data", (c) => out.push(String(c)));
pipeline(src, sink, (err) => {
  console.log("err:", err === null || err === undefined);
  console.log("joined:", out.join(""));
});
src.write("piped ");
src.write("through");
src.end();
