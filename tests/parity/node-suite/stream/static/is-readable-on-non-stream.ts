import { isReadable } from "node:stream";
// isReadable(non-stream) — returns false for plain values.
console.log("object:", isReadable({} as any));
console.log("null:", isReadable(null as any));
console.log("number:", isReadable(42 as any));
console.log("all false:", isReadable({} as any) === false && isReadable(null as any) === false);
