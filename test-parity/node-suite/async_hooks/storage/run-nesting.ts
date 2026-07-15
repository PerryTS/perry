import { AsyncLocalStorage } from "node:async_hooks";

const first = new AsyncLocalStorage<string>();
const second = new AsyncLocalStorage<{ value: number }>();

console.log(
  "initial stores:",
  String(first.getStore()),
  String(second.getStore()),
);

const returned = first.run("outer", () => {
  console.log("outer store:", first.getStore());
  console.log("second isolated:", String(second.getStore()));

  return second.run({ value: 4 }, () => {
    console.log("combined stores:", first.getStore(), second.getStore()?.value);

    const inner = first.run("inner", () => {
      console.log("inner store:", first.getStore());
      return "inner-result";
    });

    console.log("inner return:", inner);
    console.log("outer restored:", first.getStore());
    return "outer-result";
  });
});

console.log("run return:", returned);
console.log(
  "stores cleared:",
  String(first.getStore()),
  String(second.getStore()),
);
