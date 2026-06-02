import { PassThrough } from "node:stream";
// unpipe() with no argument detaches all piped sinks.
const src = new PassThrough();
const a = new PassThrough();
const b = new PassThrough();
src.pipe(a);
src.pipe(b);
src.unpipe();
console.log("detached all (no error):", true);
