import { Readable, Writable } from "node:stream";
// destroy() calls this._destroy(err, cb): a subclass method, an options.destroy,
// and a _destroy assigned after construction all run (node lib/internal/streams/destroy.js).
class R extends Readable {
  _read() {}
  _destroy(err: any, cb: any) {
    console.log("subclass R _destroy:", err === null ? "null" : String(err));
    cb(err);
  }
}
class W extends Writable {
  _write(_c: any, _e: any, cb: any) { cb(); }
  _destroy(err: any, cb: any) {
    console.log("subclass W _destroy:", err && err.message);
    cb(err);
  }
}
const r = new R();
r.on("close", () => console.log("R close"));
r.destroy();
const w = new W();
w.on("error", (e: any) => console.log("W error:", e.message));
w.on("close", () => console.log("W close"));
w.destroy(new Error("boom"));
const o = new Readable({
  read() {},
  destroy(err: any, cb: any) {
    console.log("options destroy:", err);
    cb(err);
  },
});
o.on("close", () => console.log("O close"));
o.destroy();
const a: any = new Readable({ read() {} });
a._destroy = function (err: any, cb: any) {
  console.log("assigned _destroy:", this === a);
  cb(err);
};
a.on("close", () => console.log("A close"));
a.destroy();
