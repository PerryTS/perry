import { PassThrough } from "node:stream";
// 'unpipe' fires when a previously-piped source detaches.
const src = new PassThrough();
const sink = new PassThrough();
let fired = false;
sink.on("unpipe", () => (fired = true));
src.pipe(sink);
src.unpipe(sink);
console.log("unpipe fired:", fired);
