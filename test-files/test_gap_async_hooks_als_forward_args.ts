import { AsyncLocalStorage } from "node:async_hooks";

const als = new AsyncLocalStorage<string>();

// run() forwards three trailing args and returns the callback's return value;
// the store remains readable from inside the callback.
const runRet = als.run(
  "store",
  function (...args) {
    console.log("run args:", JSON.stringify(args), "store:", als.getStore());
    return "r";
  },
  "a",
  "b",
  "c",
);
console.log("run ret:", runRet);

// run() arrow callback summing forwarded numbers.
const sum = als.run("nums", (x: number, y: number) => x + y, 2, 3);
console.log("run sum:", sum);

// run() with no extra args still works (empty forwarded list).
const none = als.run("solo", () => "ok");
console.log("run none:", none);

// exit() forwards two trailing args, clears the store inside the callback,
// returns the callback's return value, and restores the store afterward.
const exitRet = als.run("outer", () =>
  als.exit(
    function (...args) {
      console.log("exit args:", JSON.stringify(args), "store:", als.getStore());
      return "e";
    },
    "x",
    "y",
  ),
);
console.log("exit ret:", exitRet);
console.log("store after run:", als.getStore());
