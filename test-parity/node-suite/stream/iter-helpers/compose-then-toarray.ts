import { Readable, Transform } from "node:stream";
// readable.compose(transform).toArray() — chain compose with iter helper.
const r = Readable.from(["a", "b", "c"]);
const upper = new Transform({ transform(c, _e, cb) { cb(null, String(c).toUpperCase()); } });
const composed: any = (r as any).compose(upper);
const arr = await (composed as any).toArray();
console.log("result:", arr.map((c: any) => String(c)).join(","));
