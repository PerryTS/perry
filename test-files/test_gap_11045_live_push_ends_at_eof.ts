// #11045: a readable fed by live push()/write() output must end only at EOF.
// A chunk written in the same tick as `on('data')` used to stay in the
// retained buffer after being delivered; the next resume microtask replayed it
// and then emitted 'end' because the buffer had run dry — so the next write
// threw ERR_STREAM_WRITE_AFTER_END (nodemailer's sendMail, which writes the
// headers, pipes each body part in with `{ end: false }`, then writes more).
import { PassThrough, Readable, Transform } from "stream";

function trace(label: string, stream: any): void {
  stream.on("data", (chunk: any) => console.log(label, "data", JSON.stringify(String(chunk))));
  stream.on("end", () => console.log(label, "end"));
  stream.on("error", (err: any) => console.log(label, "error", err.code));
}

// Plain PassThrough: write in the listener's tick, write again later.
const pt = new PassThrough();
trace("pt", pt);
pt.write("head");
setTimeout(() => {
  pt.write("tail");
  pt.end();
}, 10);

// Readable fed by push(): must not end before push(null).
const r = new Readable({ read() {} });
trace("r", r);
r.push("a");
setTimeout(() => {
  r.push("b");
  r.push(null);
}, 20);

// Transform written before its writable side ends.
const t = new Transform({
  transform(chunk: any, _enc: any, cb: any) {
    cb(null, String(chunk).toUpperCase());
  },
});
trace("t", t);
t.write("x");
setTimeout(() => {
  t.write("y");
  t.end();
}, 30);

// nodemailer's shape: header write, body piped with { end: false }, then a
// trailer written once the body source ends.
setTimeout(() => {
  const out = new PassThrough();
  trace("mime", out);
  out.write("HDR\r\n");
  setImmediate(() => {
    const body = new PassThrough();
    body.pipe(out, { end: false });
    body.end("body");
    body.once("end", () => {
      setImmediate(() => {
        out.write("TAIL\r\n");
        out.end();
      });
    });
  });
}, 40);
