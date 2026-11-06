import * as wt from "node:worker_threads";

const originals = [globalThis.MessageChannel, globalThis.MessagePort, globalThis.BroadcastChannel];
const names = ["MessageChannel", "MessagePort", "BroadcastChannel"];
function globalReplacement() {}
let reads = 0;
globalThis.MessageChannel = globalReplacement as any;
for (const name of ["MessagePort", "BroadcastChannel"]) {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    get() { reads++; return globalReplacement; },
  });
}
let constructors: any[] = [];
try {
  constructors = [wt.MessageChannel, wt.MessagePort, wt.BroadcastChannel];
  const cjs: any = process.getBuiltinModule("worker_threads");
  console.log("cold namespace", constructors.map((value, index) => value === originals[index]).join(","));
  console.log("cold CJS", names.map((name, index) => cjs[name] === originals[index]).join(","));
  console.log("global getter reads", reads);
} finally {
  names.forEach((name, index) => Object.defineProperty(globalThis, name, {
    value: originals[index], writable: true, configurable: true,
  }));
}
console.log("prototypes", constructors.map((value, index) => value.prototype === originals[index].prototype).join(","));
try {
  const channel: any = Reflect.construct(constructors[0], []);
  channel.port1.on("message", (value: any) => {
    console.log("delivery", value);
    channel.port1.close();
    channel.port2.close();
  });
  channel.port2.postMessage("captured");
} catch (error: any) {
  console.log("construct", error.name, error.code ?? "");
}
try {
  const broadcast: any = Reflect.construct(constructors[2], ["constructor-exports"]);
  console.log("broadcast", broadcast.name);
  broadcast.close();
} catch (error: any) {
  console.log("broadcast error", error.name, error.code ?? "");
}
