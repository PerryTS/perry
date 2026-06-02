import { Readable } from "node:stream";
// Readable.prototype.iterator(options) is an explicit alias for
// [Symbol.asyncIterator]() that accepts { destroyOnReturn } to control
// whether early-return destroys the stream.
const r = Readable.from(["a", "b", "c"]);
const it = (r as any).iterator({ destroyOnReturn: false });
console.log("has next:", typeof it.next === "function");
const a = await it.next();
const b = await it.next();
console.log("first two:", a.value, b.value);
// destroyOnReturn:false → calling .return() should NOT destroy the stream.
await (it.return ? it.return() : Promise.resolve());
console.log("destroyed after early return:", r.destroyed);
