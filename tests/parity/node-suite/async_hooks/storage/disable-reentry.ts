import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

storage.run("first", () => {
  console.log("before disable:", storage.getStore());
  storage.disable();
  console.log("after disable:", String(storage.getStore()));

  storage.exit(() => {
    console.log("exit while disabled:", String(storage.getStore()));
  });

  storage.run("second", () => {
    console.log("re-entry store:", storage.getStore());
    process.nextTick(() => {
      console.log("re-entry nextTick:", storage.getStore());
    });
  });

  console.log("after re-entry:", String(storage.getStore()));
});

console.log("outside store:", String(storage.getStore()));

setImmediate(() => {
  console.log("completion store:", String(storage.getStore()));
  storage.disable();
});
