import * as vm from "node:vm";

const sandbox: any = { count: 1, nested: { value: 3 } };
const context = vm.createContext(sandbox);

console.log(
  "first:",
  vm.runInContext(
    "count = count + 1; nested.value = nested.value + 2; count",
    context,
  ),
);
console.log("after first:", sandbox.count, sandbox.nested.value);
console.log(
  "second:",
  vm.runInContext("count += 4; added = count; added", context),
);
console.log("after second:", sandbox.count, sandbox.added);
console.log(
  "same receiver:",
  vm.runInContext("this", context) === vm.runInContext("globalThis", context),
);
console.log("sandbox receiver:", vm.runInContext("this", context) === sandbox);
