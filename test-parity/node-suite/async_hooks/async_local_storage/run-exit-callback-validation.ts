import { AsyncLocalStorage } from "node:async_hooks";

const als = new AsyncLocalStorage();
const badCallbacks = [
  ["undefined", undefined],
  ["null", null],
  ["number", 0],
  ["boolean", false],
  ["string", "callback"],
];

function probe(method: string, invoke: (callback: any) => void) {
  for (const [label, callback] of badCallbacks) {
    als.enterWith("outer");
    try {
      invoke(callback);
      console.log(`${method}:${label}:no-throw:${als.getStore()}`);
    } catch (err: any) {
      console.log(`${method}:${label}:${err.name}:${err.code ?? "no-code"}:${als.getStore()}`);
    }
  }
}

probe("run", (callback) => {
  als.run("inner", callback);
});

probe("exit", (callback) => {
  als.exit(callback);
});

als.enterWith("outer");
const runReturn = als.run("inner", () => {
  console.log("run-valid-store", als.getStore());
  return "run-ok";
});
console.log("run-valid-return", runReturn, "after", als.getStore());

als.enterWith("outer");
const exitReturn = als.exit(() => {
  console.log("exit-valid-store", String(als.getStore()));
  return "exit-ok";
});
console.log("exit-valid-return", exitReturn, "after", als.getStore());
