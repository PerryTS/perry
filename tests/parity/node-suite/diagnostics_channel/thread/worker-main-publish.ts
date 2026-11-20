// Perry extension coverage for node:diagnostics_channel with perry/thread:
// a main-thread subscriber must observe a worker-originated publish exactly once.
import { subscribe, channel } from "node:diagnostics_channel";
import { spawn } from "perry/thread";

let count = 0;
let payloadValue = 0;
let payloadText = "";
let payloadArrayLen = 0;
subscribe("worker-event", (message: any) => {
  count++;
  payloadValue = message.value;
  payloadText = message.text;
  payloadArrayLen = message.items.length;
});

console.log("main has subscribers:", channel("worker-event").hasSubscribers);
const workerSawSubscribers = await spawn(() => {
  const ch = channel("worker-event");
  const sawSubscribers = ch.hasSubscribers;
  ch.publish({ value: 42, text: "from-worker", items: [1, 2, 3] });
  return sawSubscribers;
});

console.log("worker saw subscribers:", workerSawSubscribers);
console.log("main saw events:", count);
console.log("payload value:", payloadValue);
console.log("payload text:", payloadText);
console.log("payload array len:", payloadArrayLen);

if (!workerSawSubscribers) {
  throw new Error("worker channel should observe main-thread subscribers");
}
if (count !== 1) {
  throw new Error(`expected exactly one worker publish delivery, got ${count}`);
}
if (
  payloadValue !== 42 ||
  payloadText !== "from-worker" ||
  payloadArrayLen !== 3
) {
  throw new Error("worker publish payload was not deserialized correctly on main");
}

channel("worker-event").publish({ value: 7, text: "from-main", items: [4] });
console.log("main saw events after main publish:", count);
if (count !== 2) {
  throw new Error(`main publish should reach main subscriber exactly once, got ${count}`);
}
