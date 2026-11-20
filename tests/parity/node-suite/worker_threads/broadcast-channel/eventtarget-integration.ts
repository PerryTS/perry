import { BroadcastChannel } from "node:worker_threads";
import { getEventListeners } from "node:events";

const a = new BroadcastChannel("worker-eventtarget-borrow-a");
const b = new BroadcastChannel("worker-eventtarget-borrow-b");
const peerA = new BroadcastChannel("worker-eventtarget-borrow-a");
const peerB = new BroadcastChannel("worker-eventtarget-borrow-b");
const results: string[] = [];
let borrowed: (event: MessageEvent) => void;
let normal: (event: MessageEvent) => void;
const borrowedDelivery = new Promise<void>((resolve) => {
  borrowed = function (this: BroadcastChannel, event: MessageEvent) {
    results[0] = String(event.data) + ":" + (this === b ? "b" : this === a ? "a" : "other");
    resolve();
  };
  a.addEventListener.call(b, "message", borrowed);
});
const normalDelivery = new Promise<void>((resolve) => {
  normal = function (this: BroadcastChannel, event: MessageEvent) {
    results[1] = String(event.data) + ":" + (this === b ? "b" : "other");
    resolve();
  };
  b.addEventListener("message", normal);
});
peerA.postMessage("for-a");
peerB.postMessage("for-b");
Promise.all([borrowedDelivery, normalDelivery]).then(() => {
  console.log("borrowed:", results[0]);
  console.log("normal:", results[1]);
  a.removeEventListener.call(b, "message", borrowed);
  b.removeEventListener("message", normal);
  a.close();
  b.close();
  peerA.close();
  peerB.close();
  console.log("complete:", 4);
  if (typeof b.dispatchEvent !== "function") {
    console.log("EventTarget dispatch: missing");
    return;
  }
  checkEventTarget(b);
});

function checkEventTarget(channel: BroadcastChannel) {
  const add = channel.addEventListener;
  const remove = channel.removeEventListener;
  const noop = () => {};
  for (const receiver of [BroadcastChannel.prototype, Object.create(channel), new Proxy(channel, {})]) {
    add.call(receiver, "borrowed", noop);
    remove.call(receiver, "borrowed", noop);
    console.log("receiver accepted:", getEventListeners(receiver, "borrowed").length);
  }
  for (const receiver of [{}, null, undefined]) {
    try {
      add.call(receiver, "invalid", noop);
      console.log("invalid receiver accepted");
    } catch (error: any) {
      console.log("invalid receiver:", error.name, error.code);
    }
  }
  const order: string[] = [];
  channel.addEventListener("message", () => order.push("first"));
  channel.onmessage = () => order.push("handler1");
  channel.addEventListener("message", () => order.push("tail"));
  const dispatch = () => channel.dispatchEvent(new Event("message"));
  dispatch();
  channel.onmessage = () => order.push("handler2");
  dispatch();
  channel.onmessage = null;
  dispatch();
  channel.onmessage = () => order.push("handler3");
  dispatch();
  console.log("handler order:", order.join(","));
  console.log("closed listener count:", getEventListeners(channel, "message").length);
  channel.onmessageerror = null;
  console.log("initial null handler slot:", getEventListeners(channel, "messageerror").length);
  let errorHandler = 0;
  channel.onmessageerror = () => errorHandler++;
  channel.dispatchEvent(new Event("messageerror"));
  channel.onmessageerror = null;
  channel.dispatchEvent(new Event("messageerror"));
  console.log("messageerror:", errorHandler, getEventListeners(channel, "messageerror").length);

  let count = 0;
  function callback() { count++; }
  const bound = callback.bind(channel);
  channel.addEventListener("capture", bound, true);
  channel.addEventListener("capture", bound, true);
  channel.removeEventListener("capture", bound, false);
  channel.dispatchEvent(new Event("capture"));
  channel.removeEventListener("capture", bound, true);
  channel.dispatchEvent(new Event("capture"));
  console.log("capture and bound:", count, getEventListeners(channel, "capture").length);
  channel.addEventListener("once", callback, { once: true });
  channel.dispatchEvent(new Event("once"));
  channel.dispatchEvent(new Event("once"));
  console.log("once:", count, getEventListeners(channel, "once").length);
  const controller = new AbortController();
  channel.addEventListener("signal", callback, { signal: controller.signal });
  controller.abort();
  channel.dispatchEvent(new Event("signal"));
  console.log("signal:", count, getEventListeners(channel, "signal").length);
  const listener = {
    handleEvent(event: Event) {
      console.log("object listener:", this === listener, event.target === channel,
        event.currentTarget === channel, event.eventPhase);
    },
  };
  channel.addEventListener("object", listener);
  const event = new Event("object");
  channel.dispatchEvent(event);
  console.log("dispatch cleanup:", event.currentTarget === null, event.eventPhase);
  const control = new EventTarget();
  control.addEventListener("control", listener);
  control.removeEventListener("control", listener);
  console.log("EventTarget control:", control.dispatchEvent(new Event("control")));
  const abort = new AbortController();
  const abortOrder: string[] = [];
  abort.signal.addEventListener("abort", () => abortOrder.push("first"));
  abort.signal.onabort = () => abortOrder.push("handler");
  abort.signal.addEventListener("abort", () => abortOrder.push("tail"));
  abort.abort();
  console.log("onabort control:", abortOrder.join(","));

  const target = new EventTarget();
  const seen: string[] = [];
  target.addEventListener("event", () => {
    seen.push("first");
    Object.defineProperty(target, "constructor", { value: null, configurable: true });
  });
  target.addEventListener("event", () => seen.push("tail"));
  console.log("result", target.dispatchEvent(new Event("event")), seen.join(","));
}
