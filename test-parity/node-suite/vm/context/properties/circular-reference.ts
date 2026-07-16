import * as vm from "node:vm";

const sandbox: any = { value: 1 };
sandbox.self = sandbox;
const context = vm.createContext(sandbox);

console.log("outer chain:", sandbox.self.self.self === sandbox);
console.log("context read:", vm.runInContext("self.self.value", context));
vm.runInContext("self.value = 4", context);
console.log("context write:", sandbox.value, sandbox.self.value);
