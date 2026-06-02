import { PassThrough, finished } from "node:stream";
// finished(stream, cb) fires its callback once the stream completes (end /
// error / close), with `null` on clean completion.
const p = new PassThrough();
finished(p, (err) => console.log("err:", err === null || err === undefined));
p.end("done");
