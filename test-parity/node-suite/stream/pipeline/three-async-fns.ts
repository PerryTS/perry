import { Readable, pipeline } from "node:stream";
// pipeline with multiple async-generator stages.
const src = Readable.from(["a", "b"]);
async function* upper(s: AsyncIterable<any>) {
  for await (const c of s) yield String(c).toUpperCase();
}
async function* prefix(s: AsyncIterable<any>) {
  for await (const c of s) yield "<" + c + ">";
}
const collected: string[] = [];
pipeline(
  src,
  upper as any,
  prefix as any,
  async function (source: AsyncIterable<any>) {
    for await (const v of source) collected.push(String(v));
  },
  (err: any) => {
    console.log("err:", err);
    console.log("collected:", collected.join(","));
  },
);
