// Readable streams resolve `_read` when the stream is read, not when it was
// constructed (Node's `Readable.prototype.read` calls `this._read(n)`).
// light-my-request's `Request` (fastify's `app.inject`) assigns `this._read`
// AFTER `Readable.call(this)`; when Perry snapshotted `_read` at
// construction, the body was never read and a POST inject never resolved.
import { Readable } from "node:stream";
import * as util from "node:util";

const log: string[] = [];
function drain(name: string, r: any, enc?: boolean): Promise<void> {
  return new Promise((resolve) => {
    if (enc) r.setEncoding("utf8");
    let body = "";
    r.on("data", (c: any) => { body += String(c); });
    r.on("end", () => { log.push(name + " end " + JSON.stringify(body)); resolve(); });
    r.on("error", (e: any) => { log.push(name + " error " + e.code); resolve(); });
    r.resume();
  });
}

// 1. util.inherits subclass, `_read` assigned after `Readable.call`, pushes
//    from setImmediate (the light-my-request shape), read as utf8.
function Req(this: any, payload: string) {
  Readable.call(this, { autoDestroy: false });
  this._payload = payload;
  this._done = false;
  this._read = function (this: any) {
    setImmediate(() => {
      if (this._done) { this.push(null); return; }
      this._done = true;
      this.push(this._payload);
      this.push(null);
    });
  };
}
util.inherits(Req, Readable);

// 2. `_read` assigned on a plain `new Readable()` instance.
function plainAssigned(): any {
  const r: any = new Readable();
  r._read = function () { this.push("plain"); this.push(null); };
  return r;
}

// 3. options.read, then an own `_read` assigned later: the later one wins.
function optionsThenOwn(): any {
  const r: any = new Readable({ read() { this.push("options"); this.push(null); } });
  r._read = function () { this.push("own"); this.push(null); };
  return r;
}

// 4. prototype `_read` assigned AFTER an instance exists.
function Late(this: any) { Readable.call(this); }
util.inherits(Late, Readable);
const late: any = new (Late as any)();
(Late as any).prototype._read = function () { this.push("late-proto"); this.push(null); };

// 5. options.read beats a prototype `_read` (Node makes it an own property).
function Proto(this: any, opts: any) { Readable.call(this, opts); }
util.inherits(Proto, Readable);
(Proto as any).prototype._read = function () { this.push("proto"); this.push(null); };

async function main() {
  await drain("inherits-own", new (Req as any)('{"n":1}'), true);
  await drain("plain-assigned", plainAssigned());
  await drain("options-then-own", optionsThenOwn());
  await drain("late-proto", late);
  await drain("options-over-proto", new (Proto as any)({ read() { this.push("opt"); this.push(null); } }));
  await drain("proto-only", new (Proto as any)({}));
  await drain("bare", new Readable());
  for (const l of log) console.log(l);
}
main();
