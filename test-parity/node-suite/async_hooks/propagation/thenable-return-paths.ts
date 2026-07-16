import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

function makeThenable(label: string, stores: string[]) {
  return {
    then(resolve: (value: string) => void) {
      stores.push(storage.getStore() ?? "missing");
      setImmediate(() => {
        stores.push(storage.getStore() ?? "missing");
        resolve(`${label}-value`);
      });
    },
  };
}

const asyncReturnStores: string[] = [];
const asyncReturnValue = await storage.run("async-return", async () =>
  makeThenable("async-return", asyncReturnStores),
);
console.log(
  "async return thenable:",
  asyncReturnStores.join(","),
  asyncReturnValue,
);

const handlerReturnStores: string[] = [];
const handlerReturnValue = await storage.run("handler-return", () =>
  Promise.resolve()
    .then(() => makeThenable("handler-return", handlerReturnStores))
    .then((value) => {
      handlerReturnStores.push(storage.getStore() ?? "missing");
      return value;
    }),
);
console.log(
  "handler return thenable:",
  handlerReturnStores.join(","),
  handlerReturnValue,
);
console.log("thenable return outside:", String(storage.getStore()));
