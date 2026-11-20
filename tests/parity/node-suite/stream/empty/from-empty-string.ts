import { Readable } from "node:stream";
// Readable.from('') yields one empty-chunk then ends (subtly different from
// Readable.from([])).
const r = Readable.from("");
let chunks = 0;
r.on("data", () => chunks++);
r.on("end", () => console.log("chunks:", chunks));
