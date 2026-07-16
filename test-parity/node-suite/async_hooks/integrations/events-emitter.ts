import { EventEmitter } from "node:events";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

await storage.run(
  "events",
  () =>
    new Promise<void>((resolve) => {
      const emitter = new EventEmitter();
      emitter.on("sync", (value) => {
        console.log("event sync store:", storage.getStore(), value);
      });
      emitter.once("once", () => {
        console.log("event once store:", storage.getStore());
      });
      emitter.on("async", async () => {
        await Promise.resolve();
        console.log("event async store:", storage.getStore());
        resolve();
      });

      process.nextTick(() => {
        emitter.emit("sync", "value");
        emitter.emit("once");
        emitter.emit("once");
        emitter.emit("async");
      });
    }),
);

console.log("events outside:", String(storage.getStore()));
