// #3157 — MessageChannel ports + receiveMessageOnPort
// #3159 — markAsUntransferable / isMarkedAsUntransferable / markAsUncloneable
import {
  MessageChannel,
  receiveMessageOnPort,
  markAsUntransferable,
  isMarkedAsUntransferable,
  markAsUncloneable,
} from "node:worker_threads";

// --- #3159: transfer / clone markers ---
const buf = new ArrayBuffer(8);
console.log("untransferable-before:", isMarkedAsUntransferable(buf));
console.log("markAsUntransferable-return:", markAsUntransferable(buf));
console.log("untransferable-after:", isMarkedAsUntransferable(buf));
console.log("primitive-untransferable:", isMarkedAsUntransferable(42));

const obj = { kind: "uncloneable" };
console.log("markAsUncloneable-return:", markAsUncloneable(obj));

// --- #3157: synchronous receiveMessageOnPort ---
const sync = new MessageChannel();
sync.port1.postMessage({ hello: "world", n: 7 });
const received = receiveMessageOnPort(sync.port2);
console.log("receiveMessageOnPort:", JSON.stringify(received));
console.log("receiveMessageOnPort-empty:", receiveMessageOnPort(sync.port2));
sync.port1.close();
sync.port2.close();

// --- #3157: async message event delivery ---
(async () => {
  await new Promise<void>((resolve) => {
    const ch = new MessageChannel();
    let count = 0;
    ch.port2.on("message", (value: unknown) => {
      count++;
      console.log("on-message:", JSON.stringify(value));
      if (count === 2) {
        ch.port1.close();
        ch.port2.close();
        resolve();
      }
    });
    ch.port1.postMessage({ a: 1 });
    ch.port1.postMessage([2, 3, 4]);
  });
  console.log("done");
})();
