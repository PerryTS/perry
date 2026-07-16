import { EventEmitter, on } from "node:events";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const emitter = new EventEmitter();

const result = await storage.run("events-on", async () => {
  process.nextTick(() => {
    emitter.emit("value", "first");
    emitter.emit("value", "second");
    emitter.emit("end");
  });

  const output: string[] = [];
  for await (const [value] of on(emitter, "value", {
    close: ["end"],
  })) {
    console.log("events.on iterator store:", storage.getStore(), value);
    output.push(String(value));
  }
  return output.join(",");
});

console.log("events.on result:", result);
console.log("events.on outside:", String(storage.getStore()));
