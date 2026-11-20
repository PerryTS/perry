import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

let tickDone!: Promise<void>;
let timerDone!: Promise<void>;

storage.run("pending", () => {
  tickDone = new Promise<void>((resolve) => {
    process.nextTick(() => {
      console.log("disabled nextTick:", String(storage.getStore()));
      resolve();
    });
  });

  timerDone = new Promise<void>((resolve) => {
    setTimeout(() => {
      console.log("disabled timer:", String(storage.getStore()));
      resolve();
    }, 0);
  });

  storage.disable();
  console.log("disabled synchronously:", String(storage.getStore()));
});

await Promise.all([tickDone, timerDone]);
console.log("disabled pending complete:", String(storage.getStore()));
