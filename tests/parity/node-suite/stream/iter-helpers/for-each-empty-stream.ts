import { Readable } from "node:stream";
// forEach on empty stream — no calls, resolves to undefined.
const r = Readable.from([]);
let calls = 0;
const result = await (r as any).forEach((_x: any) => { calls++; });
console.log("calls:", calls);
console.log("result:", result);
