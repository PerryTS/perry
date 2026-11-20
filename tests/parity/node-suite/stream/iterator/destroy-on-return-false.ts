import { Readable } from "node:stream";
// readable.iterator({ destroyOnReturn: false }) keeps the stream open after
// the iterator's return().
const r = Readable.from(["a", "b", "c"]);
const it = (r as any).iterator({ destroyOnReturn: false });
const first = await it.next();
console.log("first value:", first.value);
await it.return();
console.log("readable after return:", r.readable);
