import * as wt from "node:worker_threads";
import { MessageChannel as MC, MessagePort as MP, BroadcastChannel as BC } from "node:worker_threads";
import { syncBuiltinESMExports } from "node:module";

const originals = [globalThis.MessageChannel, globalThis.MessagePort, globalThis.BroadcastChannel];
const names = ["MessageChannel", "MessagePort", "BroadcastChannel"];
console.log("named", MC === originals[0], MP === originals[1], BC === originals[2]);
try {
  const channel: any = Reflect.construct(MC, []);
  channel.port1.on("message", (value: any) => {
    console.log("named delivery", value);
    channel.port1.close();
    channel.port2.close();
  });
  channel.port2.postMessage("without CJS initialization");
} catch (error: any) {
  console.log("named construction", error.name, error.code ?? "");
}
function globalReplacement() {}
let reads = 0;
for (const name of names) {
  Object.defineProperty(globalThis, name, {
    configurable: true,
    get() { reads++; return globalReplacement; },
  });
}
try {
  syncBuiltinESMExports();
  console.log("global sync", MC === originals[0], MP === originals[1], BC === originals[2], reads);
} finally {
  names.forEach((name, index) => Object.defineProperty(globalThis, name, {
    value: originals[index], writable: true, configurable: true,
  }));
  syncBuiltinESMExports();
}
const cjs: any = process.getBuiltinModule("worker_threads");
function moduleReplacement() {}
cjs.MessageChannel = moduleReplacement;
try {
  console.log("before CJS sync", MC === originals[0], cjs.MessageChannel === moduleReplacement);
  syncBuiltinESMExports();
  console.log("after CJS sync", MC === moduleReplacement, globalThis.MessageChannel === originals[0]);
} finally {
  cjs.MessageChannel = originals[0];
  syncBuiltinESMExports();
}
console.log("restored", MC === originals[0], wt.MessagePort === originals[1]);
