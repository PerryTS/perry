import { Readable, PassThrough, pipeline } from "node:stream";
// pipeline accepts an async function as a middle stage that consumes the
// upstream async iterable and yields transformed chunks.
const src = Readable.from(["a", "b"]);
const sink = new PassThrough();
const out: string[] = [];
sink.on("data", (c) => out.push(String(c)));
pipeline(
  src,
  async function* (source: AsyncIterable<any>) {
    for await (const c of source) yield String(c).toUpperCase();
  },
  sink,
  (err) => {
    console.log("err:", err === null || err === undefined);
    console.log("joined:", out.join(""));
  },
);
