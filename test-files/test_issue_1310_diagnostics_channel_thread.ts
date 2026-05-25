// Regression for #1310: node:diagnostics_channel is process-global across
// perry/thread workers. A subscriber registered on the main thread must see
// publishes originating from a spawned worker; worker-side hasSubscribers
// should also observe the main-thread subscription.
import { subscribe, unsubscribe, channel } from "node:diagnostics_channel";
import { spawn } from "perry/thread";

let count = 0;
let payloadValue = 0;
let payloadText = "";
let payloadArrayLen = 0;

const cb = (message: any, name: string) => {
  if (name === "worker-event") {
    count++;
    payloadValue = message.value;
    payloadText = message.text;
    payloadArrayLen = message.items.length;
  }
};

subscribe("worker-event", cb);
console.log("main has subscribers:", channel("worker-event").hasSubscribers);
if (!channel("worker-event").hasSubscribers) {
  throw new Error("main channel should report subscribers after subscribe()");
}

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
  throw new Error("worker should observe the main-thread subscriber");
}
if (count !== 1) {
  throw new Error(`expected exactly one worker publish delivery, got ${count}`);
}
if (payloadValue !== 42 || payloadText !== "from-worker" || payloadArrayLen !== 3) {
  throw new Error("worker publish payload was not deserialized correctly on main");
}

const workerSubscribed = await spawn(() => {
  let localHits = 0;
  subscribe("worker-event", () => {
    localHits++;
  });
  return channel("worker-event").hasSubscribers;
});

console.log("worker subscribed:", workerSubscribed);
if (!workerSubscribed) {
  throw new Error("worker channel should report subscribers after worker subscribe()");
}
channel("worker-event").publish({ value: 7, text: "from-main", items: [4] });
await spawn(() => 0);
console.log("main saw events after main publish:", count);
if (count !== 2) {
  throw new Error(`main publish should reach main subscriber exactly once, got ${count}`);
}

unsubscribe("worker-event", cb);
