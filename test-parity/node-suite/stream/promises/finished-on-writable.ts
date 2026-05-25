import { Writable } from "node:stream";
import { finished } from "node:stream/promises";
// finished() on a Writable resolves when it finishes.
const w = new Writable({ write(_c, _e, cb) { cb(); } });
const p = finished(w);
w.end("done");
const result = await p;
console.log("resolved to:", result);
console.log("is undefined:", result === undefined);
