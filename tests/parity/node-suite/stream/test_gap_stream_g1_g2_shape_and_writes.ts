// STREAM-PAYLOAD-DESIGN G1/G2 (#11919): stream methods live on the
// prototypes, the runtime's stream state is never an enumerable own key, and
// writes are serialized with node's callback timing and Transform kCallback.
import { Readable, Writable, Duplex, Transform, PassThrough } from "node:stream";

// --- G1: shape ---
const all: [string, any][] = [
  ["Readable", new Readable({ read() {} })],
  ["Writable", new Writable({ write(_c, _e, cb) { cb(); } })],
  ["Duplex", new Duplex({ read() {}, write(_c, _e, cb) { cb(); } })],
  ["Transform", new Transform({ transform(c, _e, cb) { cb(null, c); } })],
  ["PassThrough", new PassThrough()],
];
for (const [name, s] of all) {
  const keys = Object.keys(s);
  const own = Object.getOwnPropertyNames(s);
  const methods = ["on", "emit", "write", "end", "push", "pipe", "destroy", "read"].filter((m) => Object.hasOwn(s, m));
  const visible = keys.filter((k) => !k.startsWith("_"));
  let internal = 0;
  for (const k in s) if (k.startsWith("__perry")) internal++;
  console.log(
    name,
    "own methods:", JSON.stringify(methods),
    "public keys:", JSON.stringify(visible),
    "perry keys:", keys.filter((k) => k.startsWith("__perry")).length, internal,
    "proto:", Object.getPrototypeOf(s) === (globalThis as any)[name] || Object.getPrototypeOf(s).constructor.name,
    "state:", keys.includes("_readableState"), keys.includes("_writableState"),
    "ownNames has write:", own.includes("write"),
  );
}
console.log(typeof (Readable.prototype as any).push, typeof (Writable.prototype as any).write, typeof (Duplex.prototype as any).write);
console.log(Object.hasOwn(Readable.prototype, "push"), Object.hasOwn(Writable.prototype, "write"), Object.hasOwn(Duplex.prototype, "write"));

class Sub extends Readable {
  _read() {}
}
const sub = new Sub();
console.log(
  "sub:",
  Object.getPrototypeOf(Sub.prototype) === Readable.prototype,
  Object.hasOwn(sub, "push"),
  typeof sub.push,
  sub instanceof Readable,
  sub.constructor === Sub,
);
// --- G2: serialized writes, async transform ---
function part1(next: () => void) {
  const log: string[] = [];
  const t = new Transform({
    transform(chunk, _enc, cb) {
      log.push("T>" + chunk);
      setTimeout(() => {
        log.push("T<" + chunk);
        cb(null, chunk);
      }, 5);
    },
  });
  t.on("data", (d) => log.push("data " + d));
  for (const x of ["a", "b", "c"]) log.push("w " + x + " -> " + t.write(x, () => log.push("cb " + x)));
  t.end(() => log.push("end cb"));
  t.on("finish", () => log.push("finish"));
  t.on("end", () => log.push("end"));
  t.on("close", () => {
    log.push("close");
    console.log(log.join("\n"));
    next();
  });
}

// --- G2: kCallback held while the readable side is full ---
function part2(next: () => void) {
  const l: string[] = [];
  const p = new Transform({
    readableHighWaterMark: 2,
    writableHighWaterMark: 2,
    transform(c, _e, cb) {
      l.push("xf " + c);
      cb(null, c);
    },
  });
  for (const x of ["1", "2", "3", "4"]) l.push("write " + x + " " + p.write(x, () => l.push("cb " + x)));
  setTimeout(() => {
    l.push("rl " + p.readableLength + " wl " + p.writableLength);
    l.push("read " + p.read(1));
    setTimeout(() => {
      l.push("rl " + p.readableLength + " wl " + p.writableLength);
      console.log(l.join("\n"));
      next();
    }, 10);
  }, 10);
}

// --- G2: a synchronous _write defers its callback; drain precedes it ---
function part3(next: () => void) {
  const l: string[] = [];
  const w = new Writable({
    highWaterMark: 3,
    write(chunk, _e, cb) {
      l.push("_write " + chunk);
      cb();
    },
  });
  w.on("drain", () => l.push("drain"));
  w.on("finish", () => {
    l.push("finish");
    console.log(l.join("\n"));
    next();
  });
  l.push("w1 " + w.write("xx", () => l.push("cb1")));
  l.push("w2 " + w.write("yy", () => l.push("cb2")));
  l.push("sync done");
  w.end();
}

// --- G2: an async _write serializes; cork/uncork batches ---
function part4(next: () => void) {
  const l: string[] = [];
  const w = new Writable({
    write(chunk, _e, cb) {
      l.push("start " + chunk);
      setTimeout(() => {
        l.push("done " + chunk);
        cb();
      }, 2);
    },
  });
  w.write("1", () => l.push("cb 1"));
  w.write("2", () => l.push("cb 2"));
  w.cork();
  w.write("3", () => l.push("cb 3"));
  l.push("corked " + w.writableCorked + " len " + w.writableLength);
  w.uncork();
  w.end("4", () => {
    l.push("end cb");
    console.log(l.join("\n"));
    next();
  });
}

setTimeout(() => {
  part1(() => part2(() => part3(() => part4(() => console.log("done")))));
}, 5);
