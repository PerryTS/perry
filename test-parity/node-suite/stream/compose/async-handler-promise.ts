import { compose, Readable } from "node:stream";
// compose() accepts an async-generator handler that awaits between yields.
const src = Readable.from(["a", "b", "c"]);
async function* slow(source: AsyncIterable<any>) {
  for await (const c of source) {
    await new Promise((resolve) => setTimeout(resolve, 5));
    yield String(c).toUpperCase();
  }
}
const composed: any = compose(src, slow);
const out: string[] = [];
composed.on("data", (c: any) => out.push(String(c)));
composed.on("end", () => console.log("composed-async:", out.join(",")));
