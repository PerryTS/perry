import { Readable } from "node:stream";
// The async iterator's .return() ends iteration early and resolves with
// { value: undefined, done: true }.
const r = Readable.from(["a", "b", "c"]);
const it = (r as any)[Symbol.asyncIterator]();
await it.next();
const ret = await it.return();
console.log("done:", ret.done);
console.log("value:", ret.value);
