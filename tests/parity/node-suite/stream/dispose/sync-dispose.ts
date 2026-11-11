import { Readable } from "node:stream";
// Some stream implementations also expose Symbol.dispose (synchronous
// disposal) for use with `using` declarations. Check whether it's defined.
const r = new Readable({ read() {} });
console.log("has Symbol.dispose:", typeof (r as any)[Symbol.dispose]);
