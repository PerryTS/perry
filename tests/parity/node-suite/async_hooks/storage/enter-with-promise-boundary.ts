import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

async function enterAfterAwait() {
  await Promise.resolve();
  storage.enterWith("after-await");
  console.log("inside after-await:", storage.getStore());
}

function enterInsideThen() {
  return Promise.resolve().then(() => {
    storage.enterWith("inside-then");
    console.log("inside then:", storage.getStore());
  });
}

async function enterBeforeAwait() {
  storage.enterWith("before-await");
  await Promise.resolve();
  console.log("inside before-await continuation:", storage.getStore());
}

await enterAfterAwait();
console.log("after enterAfterAwait:", String(storage.getStore()));

await enterInsideThen();
console.log("after enterInsideThen:", String(storage.getStore()));

await enterBeforeAwait();
console.log("after enterBeforeAwait:", storage.getStore());

storage.disable();
