import { BroadcastChannel } from "node:worker_threads";

const callable = new EventTarget();
let proxyCalls = 0;
callable.addEventListener("proxy", new Proxy(function (this: EventTarget) {
  if (this === callable) proxyCalls++;
}, {}));
callable.dispatchEvent(new Event("proxy"));
console.log("callable proxy:", proxyCalls);

const mutable = new EventTarget();
const constructor = mutable.constructor;
let onceCalls = 0;
mutable.addEventListener("once", () => {
  Object.defineProperty(mutable, "constructor", { value: null, configurable: true });
});
mutable.addEventListener("once", () => onceCalls++, { once: true });
for (let i = 0; i < 2; i++) {
  mutable.dispatchEvent(new Event("once"));
  Object.defineProperty(mutable, "constructor", { value: constructor, configurable: true });
}
console.log("mutated once:", onceCalls);

const snapshot = new EventTarget();
const snapshotConstructor = snapshot.constructor;
let reads = 0;
let snapshotCalls = 0;
snapshot.addEventListener("snapshot", () => snapshotCalls++);
Object.defineProperty(snapshot, "constructor", {
  configurable: true,
  get() {
    if (++reads === 3) throw new Error("snapshot constructor");
    return snapshotConstructor;
  },
});
const event = new Event("snapshot");
try {
  console.log("snapshot first:", snapshot.dispatchEvent(event));
} catch (error: any) {
  console.log("snapshot first threw:", error.message);
}
Object.defineProperty(snapshot, "constructor", { value: snapshotConstructor, configurable: true });
try {
  console.log("snapshot reuse:", snapshot.dispatchEvent(event), snapshotCalls);
} catch (error: any) {
  console.log("snapshot reuse threw:", error.code);
}

const removal = new EventTarget();
const order: string[] = [];
let replace = true;
const tail = () => order.push("tail");
removal.addEventListener("remove", () => {
  order.push("first");
  if (replace) {
    replace = false;
    removal.removeEventListener("remove", tail);
    removal.addEventListener("remove", tail);
  }
});
removal.addEventListener("remove", tail);
removal.dispatchEvent(new Event("remove"));
removal.dispatchEvent(new Event("remove"));
console.log("remove and re-add:", order.join(","));

process.on("uncaughtException", (error: Error) => console.log("async error:", error.message));
const manual = new BroadcastChannel("worker-eventtarget-manual-errors");
manual.close();
manual.addEventListener("message", () => { throw new Error("manual listener"); });
manual.addEventListener("message", () => console.log("manual tail"));
manual.onmessage = () => console.log("manual handler");
try {
  console.log("manual returned:", manual.dispatchEvent(new Event("message")));
} catch (error: any) {
  console.log("manual threw:", error.message);
}

const receiver = new BroadcastChannel("worker-eventtarget-transport-errors");
const sender = new BroadcastChannel("worker-eventtarget-transport-errors");
receiver.addEventListener("message", () => { throw new Error("transport listener"); });
receiver.addEventListener("message", () => console.log("transport tail"));
receiver.onmessage = () => {
  console.log("transport handler");
  receiver.close();
  sender.close();
};
sender.postMessage("deliver");
