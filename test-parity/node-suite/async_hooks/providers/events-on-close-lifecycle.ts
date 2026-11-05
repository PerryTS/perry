import { EventEmitter, on } from "node:events";

const emitter = new EventEmitter();
const unrelated = () => {};
emitter.on("value", unrelated);
emitter.on("finish", unrelated);
const iterator = on(emitter, "value", {
  close: ["end", "finish"],
})[Symbol.asyncIterator]();
const waiting = iterator.next();
emitter.emit("finish");
const closed = await waiting;
console.log("pending close:", closed.done, String(closed.value));
console.log(
  "owned cleanup:",
  emitter.listenerCount("value"),
  emitter.listenerCount("end"),
  emitter.listenerCount("finish"),
);
await iterator.return!();
emitter.emit("end");
console.log("repeated close:", (await iterator.next()).done);

const bufferedEmitter = new EventEmitter();
const buffered = on(bufferedEmitter, "value", {
  close: ["end", "finish"],
})[Symbol.asyncIterator]();
bufferedEmitter.emit("value", "buffered");
bufferedEmitter.emit("end");
console.log("buffer before done:", (await buffered.next()).value[0]);
console.log("buffer completion:", (await buffered.next()).done);
console.log(
  "all close names detached:",
  bufferedEmitter.listenerCount("end"),
  bufferedEmitter.listenerCount("finish"),
);
