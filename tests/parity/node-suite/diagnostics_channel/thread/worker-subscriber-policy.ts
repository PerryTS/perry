// Perry extension coverage for #1798: worker-owned diagnostics_channel
// subscribers are refused because perry/thread workers do not have a
// persistent pump/lifetime for later callback delivery.
import { subscribe, channel } from "node:diagnostics_channel";
import { AsyncLocalStorage } from "node:async_hooks";
import { spawn } from "perry/thread";

let mainHits = 0;
subscribe("worker-owned-policy", () => {
  mainHits++;
});

const workerResult = await spawn(() => {
  const ch = channel("worker-owned-policy");
  const sawMainSubscriber = ch.hasSubscribers;
  let subscribeRejected = false;
  let subscribeMessage = "";
  try {
    subscribe("worker-owned-policy", () => {
      // This closure would be owned by the worker arena, so it must not be kept.
    });
  } catch (e: any) {
    subscribeRejected = true;
    subscribeMessage = e && e.message;
  }

  let bindStoreRejected = false;
  let bindStoreMessage = "";
  try {
    ch.bindStore(new AsyncLocalStorage());
  } catch (e: any) {
    bindStoreRejected = true;
    bindStoreMessage = e && e.message;
  }

  ch.publish({ from: "worker" });
  return {
    sawMainSubscriber,
    subscribeRejected,
    subscribeMessageMentionsMainThread: subscribeMessage.includes("main thread"),
    bindStoreRejected,
    bindStoreMessageMentionsMainThread: bindStoreMessage.includes("main thread"),
  };
});

console.log("worker saw main subscriber:", workerResult.sawMainSubscriber);
console.log("worker subscribe rejected:", workerResult.subscribeRejected);
console.log(
  "worker subscribe error mentions main thread:",
  workerResult.subscribeMessageMentionsMainThread,
);
console.log("worker bindStore rejected:", workerResult.bindStoreRejected);
console.log(
  "worker bindStore error mentions main thread:",
  workerResult.bindStoreMessageMentionsMainThread,
);
console.log("main hits:", mainHits);

if (!workerResult.sawMainSubscriber) {
  throw new Error("worker should still observe main-thread subscribers");
}
if (!workerResult.subscribeRejected) {
  throw new Error("worker-owned diagnostics subscriber should be rejected");
}
if (!workerResult.subscribeMessageMentionsMainThread) {
  throw new Error("worker subscribe rejection should explain the main-thread policy");
}
if (!workerResult.bindStoreRejected) {
  throw new Error("worker-owned diagnostics store should be rejected");
}
if (!workerResult.bindStoreMessageMentionsMainThread) {
  throw new Error("worker bindStore rejection should explain the main-thread policy");
}
if (mainHits !== 1) {
  throw new Error(`worker publish should still reach main subscriber once, got ${mainHits}`);
}
