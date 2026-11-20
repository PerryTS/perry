import { Readable } from "node:stream";
// Readable.from(null) / from(undefined) throw because they're not iterable.
let threwNull = false;
let threwUndef = false;
try { Readable.from(null as any); } catch { threwNull = true; }
try { Readable.from(undefined as any); } catch { threwUndef = true; }
console.log("null threw:", threwNull);
console.log("undefined threw:", threwUndef);
