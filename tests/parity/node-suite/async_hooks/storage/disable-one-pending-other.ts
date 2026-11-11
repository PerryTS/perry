import { AsyncLocalStorage } from "node:async_hooks";

const first = new AsyncLocalStorage<string>();
const second = new AsyncLocalStorage<string>();

const pending = first.run("first", () =>
  second.run(
    "second",
    () =>
      new Promise<void>((resolve) => {
        setImmediate(() => {
          console.log(
            "pending isolated stores:",
            String(first.getStore()),
            String(second.getStore()),
          );
          resolve();
        });
      }),
  ),
);

first.disable();
await pending;
console.log(
  "pending isolated outside:",
  String(first.getStore()),
  String(second.getStore()),
);

second.disable();
