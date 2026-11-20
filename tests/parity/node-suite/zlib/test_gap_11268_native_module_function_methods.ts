// #11268: `Function.prototype.{bind,call,apply}` on a function taken from a
// native Node module namespace. `zlib.inflate.bind(zlib)` evaluated to
// `undefined` because `<ns>.<export>.<method>(…)` was lowered as a native
// class-static call `<export>.<method>` that no table entry backed. mongodb
// 7.0.0's `compression.ts` does exactly `zlib.inflate.bind(zlib)` at startup.
//
// The matrix covers the receiver shapes (`ns.fn`, a local, a destructured
// binding, `require(...)`) across zlib, crypto, fs, util, path and events, and
// keeps the class idioms that must stay routed (`EventEmitter.call(this)`,
// `AsyncLocalStorage.bind`) as controls.
import * as zlib from "zlib";
import * as crypto from "crypto";
import * as fs from "fs";
import * as util from "util";
import * as path from "path";
import * as events from "events";
import { EventEmitter } from "events";
import * as async_hooks from "async_hooks";
import { createRequire } from "node:module";

const req = createRequire(import.meta.url);

// --- the reported shape -------------------------------------------------
console.log("zlib", typeof zlib.inflate, typeof zlib.inflate.bind(zlib));
const packed = zlib.deflateSync(Buffer.from("perry-11268"));
const inflate = zlib.inflate.bind(zlib);
inflate(packed, (err: any, out: any) => {
  console.log("zlib.inflate bound cb", err, out.toString());
});
console.log(
  "zlib.inflateSync.call",
  zlib.inflateSync.call(zlib, packed).toString(),
  zlib.inflateSync.apply(zlib, [packed]).toString(),
);

// --- typeof over the matrix --------------------------------------------
console.log(
  "typeof bind",
  typeof crypto.createHash.bind(crypto),
  typeof fs.readFileSync.bind(fs),
  typeof util.format.bind(util),
  typeof path.join.bind(path),
  typeof events.once.bind(events),
);
console.log(
  "typeof call/apply",
  typeof zlib.inflate.call,
  typeof zlib.inflate.apply,
  typeof path.join.call,
  typeof util.format.apply,
);

// --- bound functions are callable, forward args (including partial) ----
const hash = crypto.createHash.bind(crypto);
console.log("crypto bind", hash("sha256").update("x").digest("hex"));
console.log("crypto call", crypto.createHash.call(crypto, "md5").update("x").digest("hex"));
console.log("crypto apply", crypto.createHash.apply(crypto, ["sha1"]).update("x").digest("hex"));
console.log("crypto randomUUID", crypto.randomUUID.call(crypto).length);

const exists = fs.existsSync.bind(fs, "/");
console.log("fs bind partial", exists());
console.log("fs call", fs.existsSync.call(fs, "/definitely/not/here/11268"));
console.log("fs apply", fs.statSync.apply(fs, ["/"]).isDirectory());

const fmt = util.format.bind(null, "%s-%s-%s", 1);
console.log("util bind partial", fmt(2, 3));
console.log("util call", util.format.call(util, "%d+%d", 4, 5));
console.log("util apply", util.format.apply(util, ["%s!", "hi"]));
console.log("util inspect call", util.inspect.call(util, { a: [1, 2] }));

const joinA = path.join.bind(path, "a");
console.log("path bind partial", joinA("b", "c"));
console.log("path call", path.join.call(path, "x", "y"));
console.log("path apply", path.join.apply(path, ["p", "q", "r"]));
console.log("path basename bind", path.basename.bind(path)("/tmp/file.txt", ".txt"));
console.log("path immediate", path.join.bind(path, "m")("n"));

const ee = new EventEmitter();
ee.on("tick", () => {});
console.log(
  "events getEventListeners",
  events.getEventListeners.bind(events)(ee, "tick").length,
  events.getEventListeners.call(events, ee, "tick").length,
  events.getEventListeners.apply(events, [ee, "tick"]).length,
);
const once = events.once.bind(events);
once(ee, "ready").then((vals: any[]) => console.log("events once bound", vals));
ee.emit("ready", 42, "x");
console.log(
  "events listenerCount call",
  events.EventEmitter.listenerCount.call(events.EventEmitter, ee, "ready"),
);

// --- receiver shapes: local, destructured, require ---------------------
const localJoin = path.join;
console.log("local", typeof localJoin.bind(path), localJoin.call(path, "l", "j"));
const { format } = util;
console.log("destructured", format.bind(util, "[%s]")("d"));
const { deflateSync, inflateSync } = zlib;
console.log(
  "destructured zlib",
  inflateSync.bind(zlib)(deflateSync.call(zlib, Buffer.from("dz"))).toString(),
);
const rz = req("zlib");
console.log("require zlib", typeof rz.inflate.bind(rz));
const rp = req("path");
console.log("require path", rp.join.bind(rp, "r")("q"), rp.join.call(rp, "c", "d"));
const rjoin = req("path").join.bind(null, "chain");
console.log("require chain", rjoin("x"));

// --- names survive -------------------------------------------------------
console.log("names", path.join.name, util.format.name, crypto.createHash.name);
console.log("bound names", path.join.bind(path).name, util.format.bind(util).name);

// --- controls: class idioms that must stay routed ----------------------
function Emitter(this: any) {
  events.EventEmitter.call(this);
  this.tag = "e";
}
util.inherits(Emitter, events.EventEmitter);
const em: any = new (Emitter as any)();
let seen = 0;
em.on("v", (v: number) => { seen = v; });
em.emit("v", 7);
console.log("control EventEmitter.call", em.tag, seen);
console.log("control EventEmitter.bind", typeof events.EventEmitter.bind(null));

const als = new async_hooks.AsyncLocalStorage<number>();
const boundStore = als.run(5, () => async_hooks.AsyncLocalStorage.bind(() => als.getStore()));
console.log("control AsyncLocalStorage.bind", boundStore());
