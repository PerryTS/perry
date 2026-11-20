import { ReadableStream, TransformStream } from "node:stream/web";
// pipeThrough(transform, { preventClose: true }) — closing source should
// NOT close the transform's writable side.
const rs = new ReadableStream({ start(c) { c.enqueue("x"); c.close(); } });
const ts = new TransformStream();
const out: any = rs.pipeThrough(ts, { preventClose: true });
console.log("returned ReadableStream:", out instanceof ReadableStream);
console.log("transform writable still unlocked:", ts.writable.locked === false);
