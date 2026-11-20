import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

storage.enterWith("first-enter");
console.log("first enter:", storage.getStore());
storage.disable();
storage.disable();
console.log("first disabled:", String(storage.getStore()));

storage.enterWith("second-enter");
console.log("second enter:", storage.getStore());
storage.disable();
console.log("second disabled:", String(storage.getStore()));

storage.run("first-run", () => {
  console.log("first run:", storage.getStore());
  storage.disable();
  console.log("disabled in run:", String(storage.getStore()));

  storage.run("second-run", () => {
    console.log("second run:", storage.getStore());
    storage.disable();
    console.log("disabled in second run:", String(storage.getStore()));
  });
});

console.log("repeated disable outside:", String(storage.getStore()));
