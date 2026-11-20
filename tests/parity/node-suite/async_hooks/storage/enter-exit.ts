import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

storage.enterWith("entered");
console.log("entered store:", storage.getStore());

const runResult = storage.run("run", () => {
  console.log("run store:", storage.getStore());

  const exitResult = storage.exit(
    (left: string, right: string) => {
      console.log("exit store:", String(storage.getStore()));
      console.log("exit args:", left, right);
      return `${left}-${right}`;
    },
    "a",
    "b",
  );

  console.log("exit return:", exitResult);
  console.log("run restored after exit:", storage.getStore());
  return "run-result";
});

console.log("run return:", runResult);
console.log("entered restored after run:", storage.getStore());

storage.exit(() => {
  storage.enterWith("temporary");
  console.log("enter inside exit:", storage.getStore());
});
console.log("exit restores entered:", storage.getStore());

storage.disable();
