import { Readable } from "node:stream";
import { EventEmitter } from "node:events";
// readable.wrap(oldStream) bridges a Node 0.10-era stream (EventEmitter
// emitting 'data'/'end') into a modern Readable.
const old: any = new EventEmitter();
old.pause = () => {};
old.resume = () => {};
const r = new Readable({ read() {} }).wrap(old);
const out: string[] = [];
r.on("data", (c) => out.push(String(c)));
r.on("end", () => console.log("joined:", out.join("")));
process.nextTick(() => {
  old.emit("data", "wrapped");
  old.emit("end");
});
