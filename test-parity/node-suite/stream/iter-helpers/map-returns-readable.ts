import { Readable } from "node:stream";
// readable.map(fn) returns a NEW Readable (the result is consumable as
// a stream, not just an async iterable).
const r = Readable.from([1, 2, 3]);
const mapped = r.map((x: number) => x * 10);
console.log("instanceof Readable:", mapped instanceof Readable);
console.log("has on:", typeof (mapped as any).on === "function");
const out: number[] = [];
(mapped as any).on("data", (c: any) => out.push(Number(c)));
(mapped as any).on("end", () => console.log("data via on:", out.join(",")));
