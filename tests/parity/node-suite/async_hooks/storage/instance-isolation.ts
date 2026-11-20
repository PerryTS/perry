import { AsyncLocalStorage } from "node:async_hooks";

const first = new AsyncLocalStorage<string>();
const second = new AsyncLocalStorage<string>();
const third = new AsyncLocalStorage<string>();

third.enterWith("third");

first.run("first", () => {
  second.run("second", () => {
    console.log(
      "all stores:",
      first.getStore(),
      second.getStore(),
      third.getStore(),
    );
    first.disable();
    console.log(
      "after first disable:",
      String(first.getStore()),
      second.getStore(),
      third.getStore(),
    );
  });

  console.log(
    "after second run:",
    String(first.getStore()),
    String(second.getStore()),
    third.getStore(),
  );
});

console.log(
  "outside:",
  String(first.getStore()),
  String(second.getStore()),
  third.getStore(),
);

second.disable();
third.disable();
