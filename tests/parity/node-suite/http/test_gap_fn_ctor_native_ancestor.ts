// Function-constructed objects whose prototype chain reaches a native node
// prototype (util.inherits + X.call(this)): own keys, their order, the
// prototype, events, and a chain level that intercepts a key.
import * as http from "node:http";
import * as util from "node:util";
import { EventEmitter } from "node:events";
import { Readable, Writable } from "node:stream";

function Res(this: any, req: any) {
  (http.ServerResponse as any).call(this, req);
  this.extra = 1;
}
util.inherits(Res, http.ServerResponse);
(Res as any).prototype.hello = function () { return "hi " + this.statusCode; };

const req: any = { method: "GET", httpVersionMajor: 1, httpVersionMinor: 1, headers: {} };
const a = new (Res as any)(req);
const b = new (Res as any)({ method: "HEAD", httpVersionMajor: 1, httpVersionMinor: 1, headers: {} });
console.log(Object.keys(a).join(","));
console.log(Object.getPrototypeOf(a) === (Res as any).prototype, a instanceof http.ServerResponse);
console.log(a.hello(), a._hasBody, b._hasBody, a.sendDate, a.finished, a.writableEnded, a.headersSent);
let seen = "";
a.once("finish", () => { seen += "finish;"; });
a.on("custom", (x: number) => { seen += "custom" + x + ";"; });
a.emit("custom", 7);
a.emit("custom", 8);
console.log(seen, a.listenerCount("custom"), a.listenerCount("finish"));
a.extra = 2;
a.later = 3;
console.log(a.extra, a.later, Object.keys(a).slice(-3).join(","));

// A level between with an accessor is not plain: the instance is born
// ordinary, and node's keys still come out in node's order.
function Hooked(this: any, req: any) {
  (http.ServerResponse as any).call(this, req);
}
util.inherits(Hooked, http.ServerResponse);
Object.defineProperty((Hooked as any).prototype, "label", {
  get() { return "hooked " + this.statusCode; },
  configurable: true,
});
const h = new (Hooked as any)(req);
console.log(Object.keys(h).length, Object.keys(h)[3], h.label, h instanceof http.ServerResponse);

// EventEmitter, Readable and Writable ancestors.
function Em(this: any) { EventEmitter.call(this); this.name = "em"; }
util.inherits(Em, EventEmitter);
const e = new (Em as any)();
let got = 0;
e.on("x", (v: number) => { got += v; });
e.emit("x", 2); e.emit("x", 3);
console.log(Object.keys(e).join(","), got, e instanceof EventEmitter);

function Src(this: any) { Readable.call(this, { objectMode: true }); this.n = 0; }
util.inherits(Src, Readable);
(Src as any).prototype._read = function () {};
function Sink(this: any) { Writable.call(this, { objectMode: true }); this.items = []; }
util.inherits(Sink, Writable);
(Sink as any).prototype._write = function (c: any, _e: any, cb: any) { this.items.push(c); cb(); };
const src = new (Src as any)();
const sink = new (Sink as any)();
console.log(Object.keys(src).join(","));
console.log(Object.keys(sink).join(","));
sink.on("finish", () => console.log("sink", sink.items.join(",")));
src.on("data", (c: any) => sink.write(c));
src.on("end", () => sink.end());
src.push(0); src.push(1); src.push(2); src.push(null);
