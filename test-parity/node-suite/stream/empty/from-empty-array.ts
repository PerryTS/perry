import { Readable } from "node:stream";
// Readable.from([]) is a valid empty stream — emits 'end' immediately, no
// 'data' events.
const r = Readable.from([]);
let dataFired = 0;
r.on("data", () => dataFired++);
r.on("end", () => console.log("data count:", dataFired));
