import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const receiver = { marker: "receiver" };

const bound = AsyncLocalStorage.bind(function (
  this: typeof receiver,
  left: number,
  right: number,
) {
  console.log("empty bind store:", String(storage.getStore()));
  console.log("empty bind receiver:", this === receiver);
  console.log("empty bind args:", left, right);
  return left * right;
});

storage.enterWith("caller");
console.log("empty bind result:", bound.call(receiver, 3, 4));
console.log("empty bind restored:", storage.getStore());

storage.disable();
