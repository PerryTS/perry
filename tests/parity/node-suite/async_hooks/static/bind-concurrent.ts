import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const bound = storage.run("captured", () =>
  AsyncLocalStorage.bind(async (label: string) => {
    console.log(label, "bound start:", storage.getStore());
    await Promise.resolve();
    console.log(label, "bound continuation:", storage.getStore());
    return `${label}-result`;
  }),
);

const first = storage.run("first-caller", () => bound("first"));
const second = storage.run("second-caller", () => bound("second"));

console.log("concurrent bound results:", (await first) + "," + (await second));
console.log("concurrent bound outside:", String(storage.getStore()));
