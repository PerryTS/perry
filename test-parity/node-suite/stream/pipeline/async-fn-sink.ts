import { Readable, pipeline } from "node:stream";
// pipeline(src, async fn) — fn receives the async-iterable source and
// can collect or process it; pipeline calls the cb when fn resolves.
const src = Readable.from(["a", "b", "c"]);
const collected: string[] = [];
pipeline(
  src,
  async function (source: AsyncIterable<any>) {
    for await (const v of source) collected.push(String(v));
  },
  (err: any) => {
    console.log("err:", err);
    console.log("collected:", collected.join(","));
  },
);
