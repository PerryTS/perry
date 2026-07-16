import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();

const result = await storage.run("chain", () =>
  Promise.reject(new Error("expected"))
    .catch((error) => {
      console.log("catch store:", storage.getStore(), error.message);
      return "recovered";
    })
    .finally(() => {
      console.log("finally store:", storage.getStore());
    }),
);

console.log("chain result:", result);
console.log("chain outside:", String(storage.getStore()));
