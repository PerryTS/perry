import { AsyncLocalStorage } from "node:async_hooks";

const first = new AsyncLocalStorage<string>();
const second = new AsyncLocalStorage<string>();

first.enterWith("first-outer");
second.enterWith("second-outer");

const returned = await first.exit(async () => {
  console.log("exit start:", String(first.getStore()), second.getStore());
  await Promise.resolve();
  console.log(
    "exit continuation:",
    String(first.getStore()),
    second.getStore(),
  );
  return "exit-result";
});

console.log("exit async return:", returned);
console.log("exit async restored:", first.getStore(), second.getStore());

first.disable();
second.disable();
