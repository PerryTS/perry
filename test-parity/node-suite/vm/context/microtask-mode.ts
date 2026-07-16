import * as vm from "node:vm";

const sandbox: any = { value: 1 };
vm.runInNewContext("Promise.resolve().then(() => value = 2)", sandbox, {
  microtaskMode: "afterEvaluate",
});
console.log("after evaluate:", sandbox.value);

const context: any = vm.createContext({ value: 3 }, {
  microtaskMode: "afterEvaluate",
});
vm.runInContext("Promise.resolve().then(() => value = 4)", context);
console.log("context option:", context.value);
