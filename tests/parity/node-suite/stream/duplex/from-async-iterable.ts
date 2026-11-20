import { Duplex } from "node:stream";
// Duplex.from(asyncIterable) creates a readable-side Duplex that emits
// each yielded value, just like Readable.from but with Duplex shape.
async function* gen() {
  yield "a";
  yield "b";
}
const d: any = (Duplex as any).from(gen());
const out: string[] = [];
d.on("data", (c: any) => out.push(String(c)));
d.on("end", () => {
  console.log("from async-iter:", out.join(","));
  console.log("is Duplex:", d instanceof Duplex);
});
