import { AsyncLocalStorage, AsyncResource } from "node:async_hooks";

const first = new AsyncLocalStorage<string>();
const second = new AsyncLocalStorage<string>();
const receiver = { label: "receiver" };

let boundFromStorage: (...args: number[]) => number;
let boundFromResource: (...args: string[]) => string;

first.run("captured-first", () => {
  second.run("captured-second", () => {
    boundFromStorage = AsyncLocalStorage.bind(function (
      this: typeof receiver,
      ...values: number[]
    ) {
      console.log("storage contexts:", first.getStore(), second.getStore());
      console.log("storage receiver:", this === receiver);
      console.log("storage args:", JSON.stringify(values));
      return values.reduce((sum, value) => sum + value, 0);
    });

    boundFromResource = AsyncResource.bind(function (
      this: typeof receiver,
      ...values: string[]
    ) {
      console.log("resource contexts:", first.getStore(), second.getStore());
      console.log("resource receiver:", this === receiver);
      console.log("resource args:", JSON.stringify(values));
      return values.join(":");
    }, "ParityStaticBind");
  });
});

first.enterWith("current-first");
second.enterWith("current-second");

console.log("storage return:", boundFromStorage!.call(receiver, 2, 3, 4));
console.log("storage restored:", first.getStore(), second.getStore());
console.log("resource return:", boundFromResource!.call(receiver, "a", "b"));
console.log("resource restored:", first.getStore(), second.getStore());

first.disable();
second.disable();
