import { Readable } from "node:stream";
// Stream instances don't have a custom Symbol.toPrimitive override.
const r = new Readable({ read() {} });
console.log("toPrimitive typeof:", typeof (r as any)[Symbol.toPrimitive]);
console.log("toPrimitive undefined:", (r as any)[Symbol.toPrimitive] === undefined);
