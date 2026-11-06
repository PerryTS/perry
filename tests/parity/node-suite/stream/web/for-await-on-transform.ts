import { TransformStream } from "node:stream/web";
// for-await directly over a TransformStream's readable side.
const ts = new TransformStream({
  transform(c, ctrl) { ctrl.enqueue(String(c).toUpperCase()); },
});
const writer = ts.writable.getWriter();
const out: string[] = [];
const iterPromise = (async () => {
  for await (const v of ts.readable as any) out.push(String(v));
})();
await writer.write("a");
await writer.write("b");
await writer.close();
await iterPromise;
console.log("collected:", out.join(","));
