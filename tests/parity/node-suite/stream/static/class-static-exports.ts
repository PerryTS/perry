import stream, { Duplex, PassThrough, Readable, Transform, Writable } from "node:stream";
import { ReadableStream, WritableStream } from "node:stream/web";

const checks = [
  ["Readable.from", Readable.from, 2],
  ["Readable.fromWeb", Readable.fromWeb, 2],
  ["Readable.toWeb", Readable.toWeb, 2],
  ["Readable.isDisturbed", Readable.isDisturbed, 1],
  ["Readable.isErrored", Readable.isErrored, 1],
  ["Writable.fromWeb", Writable.fromWeb, 2],
  ["Writable.toWeb", Writable.toWeb, 1],
  ["Writable.isDisturbed", Writable.isDisturbed, 1],
  ["Writable.isErrored", Writable.isErrored, 1],
  ["Duplex.from", Duplex.from, 1],
  ["Duplex.fromWeb", Duplex.fromWeb, 2],
  ["Duplex.toWeb", Duplex.toWeb, 2],
  ["Transform.from", Transform.from, 1],
  ["Transform.fromWeb", Transform.fromWeb, 2],
  ["Transform.toWeb", Transform.toWeb, 2],
  ["PassThrough.from", PassThrough.from, 1],
  ["PassThrough.fromWeb", PassThrough.fromWeb, 2],
  ["PassThrough.toWeb", PassThrough.toWeb, 2],
] as const;

for (const [name, fn, length] of checks) {
  console.log("static:", name, typeof fn, fn.length === length);
}

console.log("namespace identity:", stream.Readable.from === Readable.from);

const readableFrom = Readable.from;
const readable = readableFrom(["a", "b"]);
console.log("detached Readable.from:", typeof readable, readable.readable, readable.destroyed);
console.log("detached Readable.from values:", (await readable.toArray()).join(","));

const duplexFrom = Duplex.from;
const duplex = duplexFrom(Readable.from(["x"]));
console.log("detached Duplex.from:", typeof duplex, duplex.readable, duplex.writable);
duplex.on("error", () => {});
duplex.destroy();

const readableToWeb = Readable.toWeb;
const readableWeb = readableToWeb(Readable.from(["r"]));
console.log("detached Readable.toWeb:", typeof readableWeb.getReader);

const writableToWeb = Writable.toWeb;
const writableWeb = writableToWeb(new Writable({ write(_chunk, _enc, cb) { cb(); } }));
console.log("detached Writable.toWeb:", typeof writableWeb.getWriter);

const duplexToWeb = Duplex.toWeb;
const pair = duplexToWeb(Duplex.from(Readable.from(["d"])));
console.log(
  "detached Duplex.toWeb:",
  typeof pair.readable.getReader,
  typeof pair.writable.getWriter,
);

const fromWebReadable = Readable.fromWeb;
const readableFromWeb = fromWebReadable(new ReadableStream({
  start(controller) {
    controller.enqueue("w");
    controller.close();
  },
}));
console.log("detached Readable.fromWeb:", typeof readableFromWeb, readableFromWeb.readable);
readableFromWeb.on("error", () => {});
readableFromWeb.destroy();

const fromWebWritable = Writable.fromWeb;
const writableFromWeb = fromWebWritable(new WritableStream({
  write() {},
}));
console.log("detached Writable.fromWeb:", typeof writableFromWeb, writableFromWeb.writable);
writableFromWeb.on("error", () => {});
writableFromWeb.destroy();

console.log(
  "static state helpers:",
  Readable.isDisturbed(Readable.from([])),
  Readable.isErrored(Readable.from([])),
);
