import * as vm from "node:vm";

let stored = "unset";
const sandbox: any = {};
Object.defineProperties(sandbox, {
  getter: {
    configurable: true,
    get: () => "read",
  },
  setter: {
    configurable: true,
    get: () => stored,
    set: (value) => {
      stored = String(value);
    },
  },
});

const context = vm.createContext(sandbox);
console.log("initial:", vm.runInContext("getter + ':' + setter", context));
console.log(
  "write:",
  vm.runInContext("setter = 'written'; getter + ':' + setter", context),
);
console.log("outer:", stored, sandbox.getter, sandbox.setter);
