import { compose, Readable } from "node:stream";
// compose() accepts a single async-generator function and turns it into
// a stream pipeline. Source is a Readable; sink is the generator output.
async function* upper(src: AsyncIterable<any>) {
  for await (const c of src) yield String(c).toUpperCase();
}
const out: string[] = [];
const composed: any = compose(Readable.from(["a", "b", "c"]), upper as any);
composed.on("data", (c: any) => out.push(String(c)));
composed.on("end", () => console.log("composed:", out.join(",")));
