import { PassThrough, pipeline } from "node:stream";
// pipeline still fires its callback cleanly for an already-ended source.
const src = new PassThrough();
src.end();
const sink = new PassThrough();
pipeline(src, sink, (err) => console.log("err:", err === null || err === undefined));
