import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
storage.enterWith("outer");

let descendant!: Promise<string>;
storage.exit(() => {
  console.log("exit descendant sync:", String(storage.getStore()));
  descendant = Promise.resolve().then(() => {
    console.log("exit descendant promise:", String(storage.getStore()));
    return "descendant-result";
  });
});

console.log("exit descendant restored:", storage.getStore());
console.log("exit descendant result:", await descendant);
console.log("exit descendant final:", storage.getStore());

storage.disable();
