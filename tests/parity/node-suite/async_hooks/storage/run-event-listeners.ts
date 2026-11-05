import { AsyncLocalStorage } from "node:async_hooks";
import { EventEmitter } from "node:events";

const storage = new AsyncLocalStorage<string>();
const emitter = new EventEmitter();
const observed: string[] = [];

storage.enterWith("outer");
emitter.on("event", () => {
  observed.push(storage.getStore() ?? "missing");
  storage.run("first-listener", () => {
    observed.push(storage.getStore() ?? "missing");
  });
  observed.push(storage.getStore() ?? "missing");
});
emitter.on("event", () => {
  observed.push(storage.getStore() ?? "missing");
});

emitter.emit("event");
console.log("run listener stores:", observed.join(","));
console.log("run after emit:", storage.getStore());
storage.disable();
