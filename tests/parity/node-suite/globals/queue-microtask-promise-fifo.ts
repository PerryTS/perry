const waitImmediate = () => new Promise<void>((resolve) => setImmediate(resolve));

const fifo: string[] = [];
Promise.resolve().then(() => fifo.push("promise1"));
queueMicrotask(() => fifo.push("queue"));
Promise.resolve().then(() => fifo.push("promise2"));
await waitImmediate();
console.log("fifo:", fifo.join(","));

const nested: string[] = [];
Promise.resolve().then(() => {
  nested.push("promise1");
  queueMicrotask(() => nested.push("queue-in-promise"));
  Promise.resolve().then(() => nested.push("promise-in-promise"));
});
queueMicrotask(() => {
  nested.push("queue1");
  Promise.resolve().then(() => nested.push("promise-in-queue"));
  queueMicrotask(() => nested.push("queue-in-queue"));
});
Promise.resolve().then(() => nested.push("promise2"));
await waitImmediate();
console.log("nested:", nested.join(","));

const nextTick: string[] = [];
await new Promise<void>((resolve) => {
  setImmediate(() => {
    if (typeof process?.nextTick === "function") {
      process.nextTick(() => nextTick.push("tick"));
    }
    Promise.resolve().then(() => nextTick.push("promise"));
    queueMicrotask(() => nextTick.push("queue"));
    setImmediate(() => {
      resolve();
    });
  });
});
console.log("nextTick:", nextTick.join(","));
