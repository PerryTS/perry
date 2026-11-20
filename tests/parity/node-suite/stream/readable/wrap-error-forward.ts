import { Readable } from "node:stream";
import { EventEmitter } from "node:events";

const old: any = new EventEmitter();
old.pause = () => {};
old.resume = () => {};

const r = new Readable({ read() {} }).wrap(old);
r.on("error", (err) => console.log("wrapped error:", (err as Error).message));

process.nextTick(() => {
  old.emit("error", new Error("boom"));
});
