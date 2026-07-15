import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const completed = storage.run(
  "scheduled",
  () =>
    new Promise<string>((resolve) => {
      process.nextTick(() => {
        console.log("nextTick store:", storage.getStore());

        setTimeout(() => {
          console.log("timer store:", storage.getStore());
          resolve("done");
        }, 0);
      });
    }),
);

storage.enterWith("caller");
console.log("caller store:", storage.getStore());
console.log("barrier result:", await completed);
console.log("caller restored:", storage.getStore());

storage.disable();
