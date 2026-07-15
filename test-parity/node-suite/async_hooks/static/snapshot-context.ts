import { AsyncLocalStorage } from "node:async_hooks";

const first = new AsyncLocalStorage<string>();
const second = new AsyncLocalStorage<string>();
const receiver = { label: "runner" };

let snapshot: ReturnType<typeof AsyncLocalStorage.snapshot>;
first.run("snapshot-first", () => {
  second.run("snapshot-second", () => {
    snapshot = AsyncLocalStorage.snapshot();
  });
});

first.enterWith("current-first");
second.enterWith("current-second");

const result = snapshot!.call(
  receiver,
  function (this: unknown, left: string, right: string) {
    console.log("snapshot contexts:", first.getStore(), second.getStore());
    console.log("snapshot callback receiver undefined:", this === undefined);
    console.log("snapshot args:", left, right);
    return `${left}-${right}`;
  },
  "x",
  "y",
);

console.log("snapshot return:", result);
console.log("snapshot restored:", first.getStore(), second.getStore());

first.disable();
second.disable();
