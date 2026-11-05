// Program writes to the sealed node:events / node:stream prototypes must be
// read back: a compiled plain store, a new key read through an instance, a
// delete, and a defineProperty accessor. Each read runs before and after the
// write through the same site.
import { EventEmitter } from "node:events";
import { Readable, Writable } from "node:stream";

function readKey(o: any, k: string): any {
  return o[k];
}
function readEmit(o: any): any {
  return o.emit;
}
function readX(o: any): any {
  return o.x;
}
function readEventNames(o: any): any {
  return o.eventNames;
}
function readY(o: any): any {
  return o.y;
}

const e = new EventEmitter();
const r = new Readable({ read() {} });
const w = new Writable({
  write(_c: any, _e: any, cb: () => void) {
    cb();
  },
});

// 1. A compiled plain store to EventEmitter.prototype.emit.
const origEmit = EventEmitter.prototype.emit;
const seen: string[] = [];
for (let i = 0; i < 3; i++) console.log("emit before", readEmit(e) === origEmit, readEmit(r) === origEmit);
EventEmitter.prototype.emit = function (this: any, name: any, ...args: any[]) {
  seen.push("patched:" + String(name));
  return origEmit.apply(this, [name, ...args]);
};
for (let i = 0; i < 3; i++) console.log("emit after", readEmit(e) === origEmit, readEmit(w) === EventEmitter.prototype.emit);
e.on("x", (v: number) => seen.push("x:" + v));
e.emit("x", 1);
console.log(seen.join(","));
EventEmitter.prototype.emit = origEmit;
console.log("emit restored", readEmit(e) === origEmit, readEmit(r) === origEmit);

// 2. Readable.prototype.x = 1, read through a stream instance.
for (let i = 0; i < 3; i++) console.log("x before", readX(r), readX(w));
(Readable.prototype as any).x = 1;
for (let i = 0; i < 3; i++) console.log("x after", readX(r), readX(w));
(Readable.prototype as any).x = 2;
console.log("x again", readX(r), readKey(r, "x"));

// 3. A delete of a prototype method: Stream.prototype.eventNames falls
// through to EventEmitter.prototype.eventNames; Writable.prototype.cork goes.
const streamProto = Object.getPrototypeOf(Readable.prototype);
const streamEventNames = streamProto.eventNames;
for (let i = 0; i < 3; i++)
  console.log(
    "delete before",
    readEventNames(w) === streamEventNames,
    readEventNames(w) === EventEmitter.prototype.eventNames,
    typeof readKey(w, "cork"),
  );
console.log(delete streamProto.eventNames, delete (Writable.prototype as any).cork);
for (let i = 0; i < 3; i++)
  console.log(
    "delete after",
    readEventNames(w) === streamEventNames,
    readEventNames(w) === EventEmitter.prototype.eventNames,
    typeof readKey(w, "cork"),
    "cork" in w,
  );
console.log(JSON.stringify(w.eventNames()));

// 4. A defineProperty accessor on Readable.prototype and EventEmitter.prototype.
for (let i = 0; i < 3; i++) console.log("y before", readY(r), readY(e));
Object.defineProperty(Readable.prototype, "y", {
  get(this: any) {
    return this === r ? "r" : "other";
  },
  configurable: true,
});
Object.defineProperty(EventEmitter.prototype, "y", {
  get() {
    return 7;
  },
  configurable: true,
});
for (let i = 0; i < 3; i++) console.log("y after", readY(r), readY(e), readY(w));
console.log("done");
