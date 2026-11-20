import { MessageChannel } from "node:worker_threads";
const buffer = new ArrayBuffer(8);
const view = new DataView(buffer);
view.setUint32(0, 0x12345678, true);
view.setUint32(4, 0xfff90000, true);
const value = new Float64Array(buffer)[0];
const channel = new MessageChannel();
const done = new Promise<void>(resolve => {
  channel.port2.on("message", message => {
    if (typeof globalThis.gc === "function") globalThis.gc();
    console.log("postMessage", typeof message.value, Number.isNaN(message.value));
    channel.port1.close(); channel.port2.close(); resolve();
  });
});
channel.port1.postMessage({ value });
await done;
