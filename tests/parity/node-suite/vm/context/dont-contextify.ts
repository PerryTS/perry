import * as vm from "node:vm";

const context: any = vm.createContext(vm.constants.DONT_CONTEXTIFY);
console.log("marker:", vm.isContext(context));
console.log("identity:", context === globalThis);
console.log("receiver exposed:", vm.runInContext("this", context) === context);
console.log(
  "global identity:",
  vm.runInContext("this === globalThis", context) === true,
);
console.log(
  "mutation:",
  vm.runInContext("value = 3; value", context),
  context.value,
);
