import { PassThrough } from "node:stream";
// A Writable emits 'pipe' when something pipes into it (and 'unpipe' on detach).
const src = new PassThrough();
const sink = new PassThrough();
let pipedSrc: any = null;
sink.on("pipe", (s) => (pipedSrc = s));
src.pipe(sink);
console.log("got pipe event from src:", pipedSrc === src);
