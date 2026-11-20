import { MessageChannel, MessagePort } from "node:worker_threads";

const close = MessagePort.prototype.close;
const owner = new MessageChannel();
const target = new MessageChannel();
const port = target.port1;
const order: string[] = [];
let calls = 0;
let closeEvents = 0;
let ignoredReads = 0;
console.log("length", close.length, owner.port1.close.length);

function callback(this: any, event: any) {
  calls++;
  order.push("callback");
  console.log("callback", this === port, event.type, event.target === port);
}

port.on("close", () => {
  closeEvents++;
  order.push("before");
});
const borrowed = owner.port1.close;
console.log("return", Reflect.apply(borrowed, port, [callback]));
Reflect.apply(close, port, [callback]);
Reflect.apply(close, port, [() => order.push("distinct")]);
const ignored = { get then() { ignoredReads++; return undefined; } };
for (const value of [undefined, null, false, 1, "callback", ignored]) {
  Reflect.apply(close, port, [value]);
}
port.on("close", () => {
  order.push("after");
  Reflect.apply(close, port, [() => order.push("late")]);
});
setImmediate(() => setImmediate(() => {
  console.log("order", order.join(","));
  console.log("counts", calls, closeEvents, ignoredReads);
  owner.port1.ref();
  console.log("owner remains open", owner.port1.hasRef());
  owner.port1.close();
  owner.port2.close();
  target.port2.close();
}));
