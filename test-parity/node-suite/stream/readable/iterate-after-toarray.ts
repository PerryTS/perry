import { Readable } from "node:stream";
// After consuming a stream via toArray(), further iteration yields no
// values (the stream is drained / ended).
const r = Readable.from(["a", "b", "c"]);
const first = await (r as any).toArray();
const second: any[] = [];
for await (const v of r as any) {
  second.push(v);
}
console.log("first:", first.join(","));
console.log("second count:", second.length);
