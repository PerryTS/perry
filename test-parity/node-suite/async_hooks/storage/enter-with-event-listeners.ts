import { AsyncLocalStorage } from "node:async_hooks";
import { EventEmitter } from "node:events";

const storage = new AsyncLocalStorage<string>();
const emitter = new EventEmitter();
const observed: string[] = [];

storage.enterWith("outer");
emitter.on("event", () => {
  observed.push(storage.getStore() ?? "missing");
  storage.enterWith("first-listener");
  observed.push(storage.getStore() ?? "missing");
});
emitter.on("event", () => {
  observed.push(storage.getStore() ?? "missing");
  storage.enterWith("second-listener");
});

emitter.emit("event");
console.log("enterWith listener stores:", observed.join(","));
console.log("enterWith after emit:", storage.getStore());
storage.disable();
