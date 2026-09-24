// #11197: every Readable/Writable carries a live `_readableState` /
// `_writableState` object. undici's BodyReadable writes
// `this._readableState.dataEmitted = false` in its constructor and reads
// endEmitted / closeEmitted / ended / destroyed / errored / encoding /
// length / buffer as live stream state.
import { Readable, Writable, Duplex, PassThrough } from "node:stream";

function snap(label: string, s: any): void {
  const rs = s._readableState;
  console.log(
    label,
    JSON.stringify({
      objectMode: rs.objectMode,
      highWaterMark: rs.highWaterMark,
      length: rs.length,
      bufferLen: rs.buffer.length,
      flowing: rs.flowing,
      ended: rs.ended,
      endEmitted: rs.endEmitted,
      destroyed: rs.destroyed,
      closed: rs.closed,
      closeEmitted: rs.closeEmitted,
      errored: rs.errored === null ? null : String(rs.errored),
      encoding: rs.encoding,
      autoDestroy: rs.autoDestroy,
      emitClose: rs.emitClose,
    }),
  );
}

// 1. The shapes from the issue.
class Body extends Readable {
  constructor(opts: any) {
    super({ autoDestroy: true, read: opts.resume, highWaterMark: 64 * 1024 });
    const rs = (this as any)._readableState;
    console.log("in ctor:", typeof rs);
    // undici BodyReadable: must be accepted and must not corrupt the stream.
    rs.dataEmitted = false;
    console.log("dataEmitted after write:", rs.dataEmitted);
  }
}
const body = new Body({ resume() {} });
console.log("after ctor:", typeof (body as any)._readableState);
const plain = new Readable({ read() {} });
console.log("plain:", typeof (plain as any)._readableState, (plain as any)._readableState.highWaterMark);
class Bare extends Readable {}
console.log("bare subclass:", typeof (new Bare() as any)._readableState);
console.log("same object each read:", (plain as any)._readableState === (plain as any)._readableState);

// 2. A paused readable's buffer, then a flowing drain to 'end' and 'close'.
const r = new Readable({ read() {} });
snap("initial", r);
r.push("abc");
r.push("de");
snap("after 2 pushes", r);
const rs: any = (r as any)._readableState;
const seen: string[] = [];
for (const chunk of rs.buffer) seen.push(String(chunk));
console.log("buffer chunks:", seen.join("|"));
r.push(null);
snap("after push(null)", r);
r.on("data", (c: any) => console.log("data", String(c), "dataEmitted", rs.dataEmitted));
r.on("end", () => snap("on end", r));
r.on("close", () => {
  snap("on close", r);
  step3();
});

// 3. objectMode + destroy(err).
function step3(): void {
  const o = new Readable({ objectMode: true, read() {} });
  snap("objectMode initial", o);
  o.push({ a: 1 });
  o.push({ b: 2 });
  snap("objectMode 2 objects", o);
  o.on("error", (e: any) => console.log("error event:", e.message));
  o.on("close", () => {
    snap("objectMode destroyed", o);
    step4();
  });
  o.destroy(new Error("boom"));
}

// 4. setEncoding, pause/resume, and the writable side.
function step4(): void {
  const e = new PassThrough();
  e.setEncoding("utf8");
  const ers: any = (e as any)._readableState;
  console.log("encoding:", ers.encoding);
  e.pause();
  console.log("flowing after pause:", ers.flowing);

  const w = new Writable({
    highWaterMark: 4,
    write(_c: any, _e: any, cb: any) {
      setTimeout(cb, 1);
    },
  });
  const ws: any = (w as any)._writableState;
  console.log("writable state:", typeof ws, ws.highWaterMark, ws.objectMode, ws.length, ws.ended, ws.finished);
  const ok = w.write("hello");
  console.log("write returned", ok, "length", ws.length, "needDrain", ws.needDrain);
  w.end();
  console.log("after end: ended", ws.ended, "finished", ws.finished);
  w.on("finish", () => {
    console.log("on finish: finished", ws.finished, "length", ws.length);
  });
  w.on("close", () => {
    console.log("on close: destroyed", ws.destroyed, "closed", ws.closed);
    const d = new Duplex({ read() {}, write(_c: any, _e: any, cb: any) { cb(); } });
    const dd: any = d;
    console.log("duplex:", typeof dd._readableState, typeof dd._writableState, dd._readableState.ended, dd._writableState.ended);
  });
}
